//! InviteService: email invites, the only way to create an account; plus the
//! empty-install bootstrap invite and the `/invite/{token}` landing page.

use std::time::Duration;

use axum::Router;
use axum::extract::Path;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use html_escape::encode_text;

use crate::AppState;
use crate::accounts::{self, NewUser};
use crate::auth::{authenticate, create_session, generate_token, hash_token, internal, lwt_applied, peer_key};
use crate::codes;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::email::templates;
use crate::permissions::{INVITE_PEOPLE, require_permission};
use crate::proto::thehideout::v1::*;
use crate::services::guild::{GuildSvc, load_guild};

/// How long an invite stays valid after creation or a resend.
pub const INVITE_TTL_DAYS: i64 = 7;
/// Invites created per inviter within an hour.
pub const CREATE_USER_MAX: usize = 30;
/// Lookups / accepts per peer IP within [`PUBLIC_WINDOW`].
pub const LOOKUP_IP_MAX: usize = 30;
pub const ACCEPT_IP_MAX: usize = 20;
const PUBLIC_WINDOW: Duration = Duration::from_secs(15 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
/// Most guilds one invite may pre-join.
const MAX_GUILDS: usize = 50;

pub struct InviteSvc {
    state: AppState,
}

impl InviteSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// A stored invite.
#[derive(Debug, Clone)]
pub struct InviteRow {
    pub id: u64,
    pub email: String,
    pub token_hash: String,
    pub code_hash: String,
    pub inviter_id: Option<u64>,
    pub guild_id: Option<u64>,
    pub guild_ids: Vec<u64>,
    pub make_admin: bool,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_user_id: Option<u64>,
}

impl InviteRow {
    pub fn status(&self) -> InviteStatus {
        match self.status.as_str() {
            "accepted" => InviteStatus::INVITE_STATUS_ACCEPTED,
            "revoked" => InviteStatus::INVITE_STATUS_REVOKED,
            _ if self.expires_at <= Utc::now() => InviteStatus::INVITE_STATUS_EXPIRED,
            _ => InviteStatus::INVITE_STATUS_PENDING,
        }
    }
}

type Row = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<Vec<i64>>,
    Option<bool>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<i64>,
);

pub async fn load(state: &AppState, id: u64) -> Result<Option<InviteRow>, ConnectError> {
    let row = state
        .session
        .execute_unpaged(
            "SELECT email, token_hash, code_hash, inviter_id, guild_id, guild_ids, make_admin, status, expires_at, accepted_user_id \
             FROM invites WHERE id = ?",
            (id_to_db(id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<Row>()
        .map_err(internal)?;
    Ok(row.map(|(email, token_hash, code_hash, inviter, guild, guilds, admin, status, expires_at, accepted)| InviteRow {
        id,
        email: email.unwrap_or_default(),
        token_hash: token_hash.unwrap_or_default(),
        code_hash: code_hash.unwrap_or_default(),
        inviter_id: inviter.filter(|i| *i != 0).map(id_from_db),
        guild_id: guild.map(id_from_db),
        guild_ids: guilds.unwrap_or_default().into_iter().map(id_from_db).collect(),
        make_admin: admin.unwrap_or(false),
        status: status.unwrap_or_default(),
        expires_at: expires_at.unwrap_or_default(),
        accepted_user_id: accepted.map(id_from_db),
    }))
}

/// The invite a link token or short code refers to.
pub async fn find(state: &AppState, token_or_code: &str) -> Result<Option<InviteRow>, ConnectError> {
    let raw = token_or_code.trim();
    if raw.is_empty() || raw.len() > 128 {
        return Ok(None);
    }
    let code = codes::normalize_crockford(raw);
    let mut hashes = vec![hash_token(raw)];
    if code.len() == 8 {
        hashes.push(hash_token(&code));
    }
    for hash in hashes {
        let id = state
            .session
            .execute_unpaged("SELECT invite_id FROM invites_by_hash WHERE hash = ?", (&hash,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?;
        if let Some((id,)) = id
            && let Some(invite) = load(state, id_from_db(id)).await?
            && (invite.token_hash == hash || invite.code_hash == hash)
        {
            return Ok(Some(invite));
        }
    }
    Ok(None)
}

/// Fresh link token and code, and their hashes.
struct Secrets {
    token: String,
    code: String,
}

impl Secrets {
    fn new() -> Self {
        Self { token: generate_token(), code: codes::invite_code() }
    }

    fn hashes(&self) -> (String, String) {
        (hash_token(&self.token), hash_token(&self.code))
    }
}

async fn index_hashes(state: &AppState, id: u64, secrets: &Secrets) -> Result<(), ConnectError> {
    let (token_hash, code_hash) = secrets.hashes();
    for hash in [token_hash, code_hash] {
        state
            .session
            .execute_unpaged("INSERT INTO invites_by_hash (hash, invite_id) VALUES (?, ?)", (hash, id_to_db(id)))
            .await
            .map_err(internal)?;
    }
    Ok(())
}

async fn unindex_hashes(state: &AppState, invite: &InviteRow) -> Result<(), ConnectError> {
    for hash in [&invite.token_hash, &invite.code_hash] {
        state
            .session
            .execute_unpaged("DELETE FROM invites_by_hash WHERE hash = ?", (hash,))
            .await
            .map_err(internal)?;
    }
    Ok(())
}

/// `{PUBLIC_URL}/invite/{token}`.
pub fn invite_link(state: &AppState, token: &str) -> String {
    format!("{}/invite/{token}", state.public_base())
}

async fn guild_names(state: &AppState, ids: &[u64]) -> Result<Vec<String>, ConnectError> {
    let mut names = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(guild) = load_guild(state, id_to_db(*id)).await? {
            names.push(guild.name);
        }
    }
    Ok(names)
}

async fn inviter_user(state: &AppState, inviter: Option<u64>) -> Result<Option<User>, ConnectError> {
    match inviter {
        Some(id) => crate::access::find_user(state, id_to_db(id)).await,
        None => Ok(None),
    }
}

/// Emails the invite.
async fn send_invite(state: &AppState, invite: &InviteRow, secrets: &Secrets) -> Result<(), ConnectError> {
    let inviter = inviter_user(state, invite.inviter_id).await?;
    let guilds = guild_names(state, &invite.guild_ids).await?;
    let email = templates::invite(
        &invite.email,
        format!("invite-{}-{}", invite.id, &secrets.hashes().1[..16]),
        inviter.as_ref().map(|u| u.username.as_str()),
        &guilds,
        &invite_link(state, &secrets.token),
        &codes::format_invite_code(&secrets.code),
    );
    state.mailer.send(email);
    Ok(())
}

/// Invite as shown to admins and inviters; `secrets` (code and link) only for its creator,
/// right after it was made or sent again.
async fn to_proto(state: &AppState, invite: &InviteRow, secrets: Option<&Secrets>) -> Result<Invite, ConnectError> {
    Ok(Invite {
        id: invite.id,
        email: invite.email.clone(),
        code: secrets.map(|s| codes::format_invite_code(&s.code)),
        link: secrets.map(|s| invite_link(state, &s.token)),
        status: invite.status().into(),
        inviter: inviter_user(state, invite.inviter_id).await?.into(),
        guild_ids: invite.guild_ids.clone(),
        created_at: datetime_to_proto(crate::convert::snowflake_datetime(invite.id)).into(),
        expires_at: datetime_to_proto(invite.expires_at).into(),
        accepted_user_id: invite.accepted_user_id,
        make_admin: invite.make_admin,
        ..Default::default()
    })
}

/// A new invite.
pub struct NewInvite {
    pub email: String,
    /// `None` = made by the server (bootstrap, CLI).
    pub inviter_id: Option<u64>,
    /// The guild of a member's invite.
    pub guild_id: Option<u64>,
    /// Every guild to join (includes `guild_id`).
    pub guild_ids: Vec<u64>,
    pub make_admin: bool,
}

/// Validates, stores and emails an invite. Returns it with its plaintext link
/// token and code.
pub async fn create(state: &AppState, new: NewInvite) -> Result<(InviteRow, String, String), ConnectError> {
    let email = accounts::normalize_email(&new.email);
    accounts::check_email(&email)?;
    if accounts::id_by_email(state, &email).await?.is_some() {
        return Err(ConnectError::already_exists("an account with that email already exists"));
    }
    let id = state.snowflake.next_id();
    let secrets = Secrets::new();
    let (token_hash, code_hash) = secrets.hashes();
    let expires_at = Utc::now() + chrono::Duration::days(INVITE_TTL_DAYS);
    let invite = InviteRow {
        id,
        email,
        token_hash,
        code_hash,
        inviter_id: new.inviter_id,
        guild_id: new.guild_id,
        guild_ids: new.guild_ids,
        make_admin: new.make_admin,
        status: "pending".into(),
        expires_at,
        accepted_user_id: None,
    };
    state
        .session
        .execute_unpaged(
            "INSERT INTO invites (id, email, token_hash, code_hash, inviter_id, guild_id, guild_ids, make_admin, status, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id_to_db(id),
                &invite.email,
                &invite.token_hash,
                &invite.code_hash,
                invite.inviter_id.map(id_to_db),
                invite.guild_id.map(id_to_db),
                invite.guild_ids.iter().copied().map(id_to_db).collect::<Vec<_>>(),
                invite.make_admin,
                &invite.status,
                expires_at,
            ),
        )
        .await
        .map_err(internal)?;
    index_hashes(state, id, &secrets).await?;
    let mut scopes = vec!["all".to_owned()];
    scopes.extend(invite.guild_ids.iter().map(|g| format!("guild:{g}")));
    for scope in scopes {
        state
            .session
            .execute_unpaged("INSERT INTO invites_by_scope (scope, id) VALUES (?, ?)", (scope, id_to_db(id)))
            .await
            .map_err(internal)?;
    }
    send_invite(state, &invite, &secrets).await?;
    crate::admin::audit(state, invite.inviter_id.unwrap_or(0), "invite.create", &format!("invite:{id}"), &invite.email).await?;
    tracing::info!(invite_id = id, "invite created");
    Ok((invite, secrets.token, secrets.code))
}

/// On an install without accounts, creates a site-admin invite for
/// `THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` (unless one is still pending) and logs it.
pub async fn bootstrap(state: &AppState, email: Option<&str>) -> Result<(), ConnectError> {
    if !accounts::none_exist(state).await? {
        return Ok(());
    }
    let Some(email) = email else {
        tracing::warn!(
            "no accounts yet: set THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL or run `thehideout-server invite <email> --admin` to invite the first admin"
        );
        return Ok(());
    };
    let pending = scope_page(state, "all", None, MAX_LIMIT).await?;
    for id in pending {
        if let Some(invite) = load(state, id).await?
            && invite.email == accounts::normalize_email(email)
            && invite.make_admin
            && invite.status() == InviteStatus::INVITE_STATUS_PENDING
        {
            tracing::info!(
                invite_id = id,
                "bootstrap admin invite is still pending (check the inbox, or run `thehideout-server invite <email> --admin` for a new code)"
            );
            return Ok(());
        }
    }
    let (invite, token, code) = create(
        state,
        NewInvite { email: email.to_owned(), inviter_id: None, guild_id: None, guild_ids: Vec::new(), make_admin: true },
    )
    .await?;
    tracing::info!(
        invite_id = invite.id,
        code = %codes::format_invite_code(&code),
        link = %invite_link(state, &token),
        "created bootstrap admin invite"
    );
    Ok(())
}

/// Invite ids of `scope`, newest first, older than `before`.
async fn scope_page(state: &AppState, scope: &str, before: Option<u64>, limit: usize) -> Result<Vec<u64>, ConnectError> {
    let result = match before {
        Some(before) => {
            state
                .session
                .execute_unpaged(
                    "SELECT id FROM invites_by_scope WHERE scope = ? AND id < ? LIMIT ?",
                    (scope, id_to_db(before), limit as i32),
                )
                .await
        }
        None => {
            state
                .session
                .execute_unpaged("SELECT id FROM invites_by_scope WHERE scope = ? LIMIT ?", (scope, limit as i32))
                .await
        }
    }
    .map_err(internal)?;
    Ok(result
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64,)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .into_iter()
        .map(|(id,)| id_from_db(id))
        .collect())
}

async fn is_site_admin(state: &AppState, user_id: u64) -> Result<bool, ConnectError> {
    Ok(accounts::load(state, user_id).await?.is_some_and(|a| a.is_admin))
}

impl InviteSvc {
    /// The invite `id` if the caller may manage it: site admins, its inviter,
    /// and INVITE_PEOPLE holders of its guild. NOT_FOUND otherwise.
    async fn manageable(&self, caller: u64, id: u64) -> Result<InviteRow, ConnectError> {
        let not_found = || ConnectError::not_found("invite not found");
        let invite = load(&self.state, id).await?.ok_or_else(not_found)?;
        if invite.inviter_id == Some(caller) || is_site_admin(&self.state, caller).await? {
            return Ok(invite);
        }
        if let Some(guild_id) = invite.guild_id
            && require_permission(&self.state, caller, guild_id, INVITE_PEOPLE).await.is_ok()
        {
            return Ok(invite);
        }
        Err(not_found())
    }
}

fn not_pending(status: InviteStatus) -> ConnectError {
    ConnectError::failed_precondition(match status {
        InviteStatus::INVITE_STATUS_ACCEPTED => "invite has already been used",
        InviteStatus::INVITE_STATUS_REVOKED => "invite was revoked",
        InviteStatus::INVITE_STATUS_EXPIRED => "invite has expired",
        _ => "invite cannot be used",
    })
}

impl InviteService for InviteSvc {
    async fn create_invite(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CreateInviteRequest>,
    ) -> ServiceResult<CreateInviteResponse> {
        let caller = authenticate(&self.state, &ctx).await?;
        self.state.limiter.check(&format!("invite:user:{caller}"), CREATE_USER_MAX, HOUR)?;
        let admin = is_site_admin(&self.state, caller).await?;
        let extra: Vec<u64> = req.guild_ids.iter().copied().collect();
        if (!extra.is_empty() || req.make_admin) && !admin {
            return Err(ConnectError::permission_denied("only site admins can pre-join guilds or invite admins"));
        }
        match req.guild_id {
            Some(guild_id) if !admin => {
                require_permission(&self.state, caller, guild_id, INVITE_PEOPLE).await?;
            }
            None if !admin => return Err(ConnectError::permission_denied("site admin rights or a guild_id required")),
            _ => {}
        }
        let mut guild_ids: Vec<u64> = req.guild_id.into_iter().chain(extra).collect();
        guild_ids.sort_unstable();
        guild_ids.dedup();
        if guild_ids.len() > MAX_GUILDS {
            return Err(ConnectError::invalid_argument("too many guilds"));
        }
        for id in &guild_ids {
            if load_guild(&self.state, id_to_db(*id)).await?.is_none() {
                return Err(ConnectError::not_found("guild not found"));
            }
        }
        let (invite, token, code) = create(
            &self.state,
            NewInvite {
                email: req.email.to_owned(),
                inviter_id: Some(caller),
                guild_id: req.guild_id,
                guild_ids,
                make_admin: req.make_admin,
            },
        )
        .await?;
        let invite = to_proto(&self.state, &invite, Some(&Secrets { token, code })).await?;
        Response::ok(CreateInviteResponse { invite: invite.into(), ..Default::default() })
    }

    async fn list_invites(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListInvitesRequest>,
    ) -> ServiceResult<ListInvitesResponse> {
        let caller = authenticate(&self.state, &ctx).await?;
        let admin = is_site_admin(&self.state, caller).await?;
        let scope = match req.guild_id {
            Some(guild_id) => {
                if !admin {
                    require_permission(&self.state, caller, guild_id, INVITE_PEOPLE).await?;
                }
                format!("guild:{guild_id}")
            }
            None if admin => "all".to_owned(),
            None => return Err(ConnectError::permission_denied("site admin rights required")),
        };
        let limit = match req.limit as usize {
            0 => DEFAULT_LIMIT,
            n => n.min(MAX_LIMIT),
        };
        let before = if req.page_token.is_empty() {
            None
        } else {
            Some(req.page_token.parse::<u64>().map_err(|_| ConnectError::invalid_argument("invalid page_token"))?)
        };
        let mut ids = scope_page(&self.state, &scope, before, limit + 1).await?;
        let more = ids.len() > limit;
        ids.truncate(limit);
        let next_page_token = if more { ids.last().map(u64::to_string).unwrap_or_default() } else { String::new() };
        let mut invites = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(invite) = load(&self.state, id).await? {
                invites.push(to_proto(&self.state, &invite, None).await?);
            }
        }
        Response::ok(ListInvitesResponse { invites, next_page_token, ..Default::default() })
    }

    async fn revoke_invite(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RevokeInviteRequest>,
    ) -> ServiceResult<RevokeInviteResponse> {
        let caller = authenticate(&self.state, &ctx).await?;
        let invite = self.manageable(caller, req.invite_id).await?;
        let applied = lwt_applied(
            self.state
                .session
                .execute_unpaged("UPDATE invites SET status = 'revoked' WHERE id = ? IF status = 'pending'", (id_to_db(invite.id),))
                .await
                .map_err(internal)?,
        )?;
        if !applied {
            return Err(not_pending(invite.status()));
        }
        crate::admin::audit(&self.state, caller, "invite.revoke", &format!("invite:{}", invite.id), &invite.email).await?;
        let invite = load(&self.state, invite.id).await?.ok_or_else(|| ConnectError::not_found("invite not found"))?;
        let invite = to_proto(&self.state, &invite, None).await?;
        Response::ok(RevokeInviteResponse { invite: invite.into(), ..Default::default() })
    }

    async fn resend_invite(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ResendInviteRequest>,
    ) -> ServiceResult<ResendInviteResponse> {
        let caller = authenticate(&self.state, &ctx).await?;
        self.state.limiter.check(&format!("invite:user:{caller}"), CREATE_USER_MAX, HOUR)?;
        let invite = self.manageable(caller, req.invite_id).await?;
        if invite.status != "pending" {
            return Err(not_pending(invite.status()));
        }
        let secrets = Secrets::new();
        let (token_hash, code_hash) = secrets.hashes();
        let expires_at = Utc::now() + chrono::Duration::days(INVITE_TTL_DAYS);
        let applied = lwt_applied(
            self.state
                .session
                .execute_unpaged(
                    "UPDATE invites SET token_hash = ?, code_hash = ?, expires_at = ? WHERE id = ? IF status = 'pending'",
                    (&token_hash, &code_hash, expires_at, id_to_db(invite.id)),
                )
                .await
                .map_err(internal)?,
        )?;
        if !applied {
            return Err(ConnectError::failed_precondition("invite cannot be used"));
        }
        unindex_hashes(&self.state, &invite).await?;
        index_hashes(&self.state, invite.id, &secrets).await?;
        let invite = load(&self.state, invite.id).await?.ok_or_else(|| ConnectError::not_found("invite not found"))?;
        send_invite(&self.state, &invite, &secrets).await?;
        crate::admin::audit(&self.state, caller, "invite.resend", &format!("invite:{}", invite.id), &invite.email).await?;
        let invite = to_proto(&self.state, &invite, Some(&secrets)).await?;
        Response::ok(ResendInviteResponse { invite: invite.into(), ..Default::default() })
    }

    async fn lookup_invite(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, LookupInviteRequest>,
    ) -> ServiceResult<LookupInviteResponse> {
        self.state
            .limiter
            .check(&format!("invite-lookup:ip:{}", peer_key(&ctx)), LOOKUP_IP_MAX, PUBLIC_WINDOW)?;
        let invite = find(&self.state, req.token_or_code)
            .await?
            .ok_or_else(|| ConnectError::not_found("invite not found"))?;
        Response::ok(LookupInviteResponse {
            masked_email: accounts::mask_email(&invite.email),
            inviter: inviter_user(&self.state, invite.inviter_id).await?.into(),
            guild_names: guild_names(&self.state, &invite.guild_ids).await?,
            expires_at: datetime_to_proto(invite.expires_at).into(),
            status: invite.status().into(),
            ..Default::default()
        })
    }

    async fn accept_invite(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, AcceptInviteRequest>,
    ) -> ServiceResult<AcceptInviteResponse> {
        self.state
            .limiter
            .check(&format!("invite-accept:ip:{}", peer_key(&ctx)), ACCEPT_IP_MAX, PUBLIC_WINDOW)?;
        let invite = find(&self.state, req.token_or_code)
            .await?
            .ok_or_else(|| ConnectError::not_found("invite not found"))?;
        if invite.status() != InviteStatus::INVITE_STATUS_PENDING {
            return Err(not_pending(invite.status()));
        }
        accounts::check_username(&accounts::normalize_username(req.username))?;
        if let Some(password) = req.password {
            accounts::check_password(password)?;
        }
        // Claim the invite first so two accepts cannot both create accounts;
        // give it back if the account cannot be created.
        let claimed = lwt_applied(
            self.state
                .session
                .execute_unpaged("UPDATE invites SET status = 'accepting' WHERE id = ? IF status = 'pending'", (id_to_db(invite.id),))
                .await
                .map_err(internal)?,
        )?;
        if !claimed {
            return Err(ConnectError::failed_precondition("invite has already been used"));
        }
        let created = accounts::create_user(
            &self.state,
            NewUser { username: req.username, password: req.password, email: Some(&invite.email) },
        )
        .await;
        let mut account = match created {
            Ok(account) => account,
            Err(e) => {
                if let Err(err) = self
                    .state
                    .session
                    .execute_unpaged("UPDATE invites SET status = 'pending' WHERE id = ? IF status = 'accepting'", (id_to_db(invite.id),))
                    .await
                {
                    tracing::warn!(invite_id = invite.id, error = %err, "failed to release invite");
                }
                return Err(e);
            }
        };
        self.state
            .session
            .execute_unpaged(
                "UPDATE invites SET status = 'accepted', accepted_user_id = ? WHERE id = ?",
                (id_to_db(account.id), id_to_db(invite.id)),
            )
            .await
            .map_err(internal)?;
        if invite.make_admin && !account.is_admin {
            crate::admin::set_flag(&self.state, account.id, true).await?;
            account.is_admin = true;
        }
        let guilds = GuildSvc::new(self.state.clone());
        for guild_id in &invite.guild_ids {
            match load_guild(&self.state, id_to_db(*guild_id)).await? {
                // `admit` refuses banned users; the account itself still gets created.
                Some(guild) => match guilds.admit(&guild, account.id).await {
                    Err(e) if e.code == connectrpc::ErrorCode::PermissionDenied => {
                        tracing::info!(guild_id, user_id = account.id, "banned user not admitted by invite")
                    }
                    other => other?,
                },
                None => tracing::info!(guild_id, "invite guild no longer exists"),
            }
        }
        crate::admin::audit(&self.state, account.id, "invite.accept", &format!("invite:{}", invite.id), &invite.email).await?;
        let token = create_session(&self.state, account.id, req.device_name).await?;
        Response::ok(AcceptInviteResponse { token, user: account.to_proto().into(), ..Default::default() })
    }
}

/// `GET /invite/{token}`: a landing page for the emailed link.
pub fn routes() -> Router<AppState> {
    Router::new().route("/invite/{token}", get(landing))
}

async fn landing(Path(token): Path<String>) -> impl IntoResponse {
    let token = encode_text(&token);
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\
         <title>thehideout invite</title></head>\
         <body style=\"font-family:-apple-system,Segoe UI,sans-serif;max-width:480px;margin:15vh auto;padding:0 16px;color:#1b1b1f\">\
         <h2>You're invited to thehideout</h2>\
         <p>Open the thehideout app, choose <b>Accept an invite</b> and paste this invite (or the code from your email):</p>\
         <p><code style=\"word-break:break-all;font-size:14px\">{token}</code></p></body></html>"
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            // No scripts or remote content on this page; inline style attributes only.
            (header::HeaderName::from_static("content-security-policy"), "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'"),
            (header::HeaderName::from_static("x-frame-options"), "DENY"),
            (header::HeaderName::from_static("x-content-type-options"), "nosniff"),
        ],
        html,
    )
}

#[cfg(test)]
mod landing_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn landing_page_has_security_headers_and_escapes_the_token() {
        let resp = landing(Path("<script>alert(1)</script>".into())).await.into_response();
        let headers = resp.headers();
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert!(headers.get("content-security-policy").unwrap().to_str().unwrap().contains("frame-ancestors 'none'"));
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("&lt;script&gt;"), "token is escaped: {body}");
        assert!(!body.contains("<script>"), "no raw injection: {body}");
    }
}
