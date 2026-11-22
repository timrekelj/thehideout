//! What the UI asks for and what comes back. The UI only pushes [`Command`]s and applies
//! [`Reply`]s (see `apply.rs`); a [`Backend`] turns one into the other. The fake backend in
//! `fake.rs` answers deterministically; `real.rs` talks to the server through `thehideout-api`.

use std::collections::HashMap;

use std::path::PathBuf;

use thehideout_api::{
    ActivityPoint, Ban, AdminFileKind, AdminFileRow, AdminGuildRow, AdminUserRow, ApiError, AuditEntry,
    Channel, ChannelIconRef, ChannelKind, DeviceSession, DmChannel, Emoji, GatewayUpdate, Guild, GuildLayout,
    Invite, InvitePreview, Member, Message, NewInvite, Overview, Page, Paged, PaletteColor, Passkey,
    PendingAttachment, Permissions, ReactionEmoji, Role, RoleList, SecondFactorMethod, Section, SecuritySummary,
    TermsStatus, TotpSetup, User, VoiceJoin, VoiceState,
};

/// What a picked picture is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePurpose {
    Avatar,
    GuildIcon {
        guild: u64,
    },
    /// A custom emoji to add to `guild`.
    Emoji {
        guild: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    // ---- signing in: no session yet, sent to the auth form's server --------------------------
    /// `username` is a username or an email.
    Login {
        username: String,
        password: String,
    },
    /// Emails a sign-in code; always succeeds (whether or not the address has an account).
    RequestEmailCode {
        email: String,
    },
    VerifyEmailCode {
        email: String,
        code: String,
    },
    /// Answers a `Reply::SecondFactor` challenge with an authenticator or (`recovery`) a recovery code.
    CompleteSecondFactor {
        challenge: String,
        code: String,
        recovery: bool,
    },
    /// Answers a `Reply::SecondFactor` challenge with a passkey (offered when the account has
    /// one): `Reply::PasskeyCeremony`, then the backend waits like `BeginPasskeyLogin` and
    /// answers `Auth`.
    BeginSecondFactorPasskey {
        challenge: String,
    },
    /// Emails a password reset code; always succeeds.
    RequestPasswordReset {
        email: String,
    },
    ResetPassword {
        email: String,
        code: String,
        new_password: String,
    },
    /// Starts a passkey sign-in: `Reply::PasskeyCeremony` brings the page to open in the browser,
    /// then the backend waits (polling once a second, up to 2 minutes) and answers `Auth`.
    BeginPasskeyLogin {
        username: String,
    },
    /// Stops waiting for the passkey ceremony under way (sign-in or a new passkey); nothing more
    /// is reported for it.
    CancelPasskey,
    /// What an invite (its code or link) is for.
    LookupInvite {
        code: String,
    },
    /// Creates the invite's account and signs in (`Auth`); `password: None` = passwordless.
    AcceptInvite {
        code: String,
        username: String,
        password: Option<String>,
        /// The terms version the invitee accepted (`InvitePreview::terms_version`); "" = not accepted.
        terms_version: String,
        /// They confirmed being at least `InvitePreview::minimum_age`.
        age_confirmed: bool,
    },
    // ---- account security -------------------------------------------------------------------
    /// Passkeys, sessions and how the account is protected: `Reply::Security`.
    LoadSecurity,
    /// `current` is "" for an account without a password; other sessions are signed out.
    ChangePassword {
        current: String,
        new: String,
    },
    /// Sends a code to `email` (also adds an email to an account without one).
    ChangeEmail {
        email: String,
        password: String,
    },
    ConfirmEmailChange {
        code: String,
    },
    BeginTotpSetup,
    ConfirmTotp {
        code: String,
    },
    /// With an authenticator or a recovery code.
    DisableTotp {
        code: String,
    },
    RegenerateRecoveryCodes {
        code: String,
    },
    /// Adds a passkey through the browser ceremony: `PasskeyCeremony`, then `PasskeyAdded`.
    BeginPasskeyRegistration {
        name: String,
    },
    RenamePasskey {
        id: u64,
        name: String,
    },
    DeletePasskey {
        id: u64,
    },
    RevokeSession {
        id: u64,
    },
    RevokeOtherSessions,
    // ---- invites ----------------------------------------------------------------------------
    CreateInvite {
        invite: NewInvite,
    },
    /// A page of invites: all (`guild: None`, admins) or one server's. `epoch` as for the admin lists.
    ListInvites {
        epoch: u64,
        guild: Option<u64>,
        page_token: Option<String>,
        limit: u32,
    },
    RevokeInvite {
        id: u64,
    },
    /// Emails the invite again with a new code and expiry.
    ResendInvite {
        id: u64,
    },
    // ---- the rest -----------------------------------------------------------------------------
    /// Me, guilds, DMs and every guild's channels, in one answer.
    LoadInitial,
    /// `LoadInitial` again after a gateway gap: same data, but it must not reset the UI.
    Refresh,
    LoadChannels {
        guild: u64,
    },
    /// Newest page when `before` is `None`, else the page older than that message id.
    LoadMessages {
        channel: u64,
        before: Option<u64>,
    },
    Send {
        channel: u64,
        content: String,
        nonce: String,
    },
    /// `Send` with uploaded files linked to the message (`attachments` are `PendingAttachment` ids).
    SendWith {
        channel: u64,
        content: String,
        nonce: String,
        attachments: Vec<u64>,
    },
    /// Streams a file from disk to the server off the UI thread, reporting `UploadProgress`, then
    /// `Uploaded`. `id` is the client's name for the upload (its tray chip).
    UploadFile {
        channel: u64,
        id: u64,
        path: PathBuf,
        filename: String,
        content_type: String,
        size: u64,
    },
    /// The same for bytes already in memory (a pasted picture).
    UploadBytes {
        channel: u64,
        id: u64,
        filename: String,
        content_type: String,
        bytes: Vec<u8>,
    },
    /// Stops an upload; nothing more is reported for it.
    CancelUpload {
        id: u64,
    },
    /// Streams a stored file to `path`, reporting `DownloadProgress`, then `Downloaded`.
    Download {
        key: String,
        path: PathBuf,
    },
    CreateGuild {
        name: String,
    },
    JoinGuild {
        code: String,
    },
    CreateChannel {
        guild: u64,
        name: String,
        kind: ChannelKind,
        section: Option<u64>,
        icon: ChannelIconRef,
        color: PaletteColor,
    },
    // ---- voice ------------------------------------------------------------------------------
    /// Joins a voice channel (leaving the one we are in): `Reply::VoiceJoined`.
    JoinVoice {
        channel: u64,
    },
    /// Leaves voice (a no-op when in none): `Reply::VoiceLeft`.
    LeaveVoice,
    /// Our own mute and deafen flags, while in a voice channel: `Reply::VoiceFlags`.
    UpdateVoiceState {
        self_mute: bool,
        self_deaf: bool,
    },
    /// Our camera and LIVE (screen share) flags, while in a voice channel; `None` leaves one
    /// unchanged: `Reply::VoiceFlags`.
    UpdateVideoState {
        self_video: Option<bool>,
        self_stream: Option<bool>,
    },
    /// Needs MUTE_MEMBERS and a higher top role: `Reply::VoiceModerated`.
    ServerMuteMember {
        guild: u64,
        user: u64,
        muted: bool,
    },
    /// Needs MOVE_MEMBERS and a higher top role; the member's client rejoins on `voice_moved`.
    MoveMember {
        guild: u64,
        user: u64,
        channel: u64,
    },
    /// Needs MOVE_MEMBERS and a higher top role.
    DisconnectMember {
        guild: u64,
        user: u64,
    },
    // ---- moderation and structure -------------------------------------------------------------
    /// Replaces the content of our own message: `Reply::MessageEdited`.
    EditMessage {
        channel: u64,
        message: u64,
        content: String,
    },
    /// Our own message, or anyone's with MANAGE_MESSAGES: `Reply::MessageDeleted`.
    DeleteMessage {
        channel: u64,
        message: u64,
    },
    /// Needs KICK_MEMBERS and a higher top role: `Reply::Kicked`.
    KickMember {
        guild: u64,
        user: u64,
    },
    /// Needs BAN_MEMBERS and a higher top role; `delete_seconds` (at most 7 days) also deletes
    /// their recent messages: `Reply::Banned`.
    BanMember {
        guild: u64,
        user: u64,
        reason: String,
        delete_seconds: Option<u32>,
    },
    /// Needs BAN_MEMBERS: `Reply::Unbanned`.
    UnbanMember {
        guild: u64,
        user: u64,
    },
    /// The guild's bans (needs BAN_MEMBERS): `Reply::Bans`.
    LoadBans {
        guild: u64,
    },
    /// Not for the owner: `Reply::GuildLeft`.
    LeaveGuild {
        guild: u64,
    },
    /// Needs MANAGE_CHANNELS: `Reply::ChannelDeleted`.
    DeleteChannel {
        guild: u64,
        channel: u64,
    },
    /// Owner only; `confirm_name` must be the guild's name: `Reply::GuildDeleted`.
    DeleteGuild {
        guild: u64,
        confirm_name: String,
    },
    /// Needs MANAGE_SERVER: `Reply::InviteRotated` with the new code.
    RotateInviteCode {
        guild: u64,
    },
    /// Owner only, to another member; `confirm_name` must be the guild's name:
    /// `Reply::OwnershipTransferred`.
    TransferOwnership {
        guild: u64,
        user: u64,
        confirm_name: String,
    },
    /// The guild's roles and who holds which: what decides the controls shown.
    LoadAccess {
        guild: u64,
    },
    CreateSection {
        guild: u64,
        name: String,
    },
    RenameSection {
        guild: u64,
        section: u64,
        name: String,
    },
    DeleteSection {
        guild: u64,
        section: u64,
    },
    /// The desired order, based on `layout.version`; a stale one is refused with `Conflict`.
    UpdateLayout {
        guild: u64,
        layout: GuildLayout,
    },
    /// `None` fields stay as they are; empty icon/topic and `PaletteColor::None` clear.
    UpdateChannel {
        guild: u64,
        channel: u64,
        name: Option<String>,
        icon: Option<ChannelIconRef>,
        color: Option<PaletteColor>,
        topic: Option<String>,
    },
    LoadMembers {
        guild: u64,
    },
    OpenDm {
        user_id: u64,
    },
    /// Downloads a stored picture (`User::avatar`, `Guild::icon`).
    FetchMedia {
        key: String,
    },
    /// Reads a picked file off the UI thread: answered with `ImageRead`.
    ReadImage {
        path: PathBuf,
        purpose: ImagePurpose,
    },
    UploadAvatar {
        bytes: Vec<u8>,
        content_type: String,
    },
    RemoveAvatar,
    UploadGuildIcon {
        guild: u64,
        bytes: Vec<u8>,
        content_type: String,
    },
    RemoveGuildIcon {
        guild: u64,
    },
    RenameGuild {
        guild: u64,
        name: String,
    },
    CreateRole {
        guild: u64,
        name: String,
        color: PaletteColor,
        permissions: Permissions,
    },
    /// `None` fields stay as they are.
    UpdateRole {
        guild: u64,
        role: u64,
        name: Option<String>,
        color: Option<PaletteColor>,
        permissions: Option<Permissions>,
    },
    DeleteRole {
        guild: u64,
        role: u64,
    },
    /// The full order, most senior first, based on `expected_version`; a stale one is `Conflict`.
    ReorderRoles {
        guild: u64,
        expected_version: u64,
        role_ids: Vec<u64>,
    },
    /// Replaces the roles a member holds.
    SetMemberRoles {
        guild: u64,
        user: u64,
        role_ids: Vec<u64>,
    },
    /// The guild's custom emojis.
    LoadEmojis {
        guild: u64,
    },
    /// Uploads a custom emoji (PNG, JPEG, WebP or GIF, at most 2 MiB; the name is `[a-z0-9_]{2,32}`).
    CreateEmoji {
        guild: u64,
        name: String,
        bytes: Vec<u8>,
        content_type: String,
    },
    RenameEmoji {
        guild: u64,
        emoji: u64,
        name: String,
    },
    DeleteEmoji {
        guild: u64,
        emoji: u64,
    },
    /// Adds your reaction (idempotent).
    AddReaction {
        channel: u64,
        message: u64,
        emoji: ReactionEmoji,
    },
    /// Takes your reaction back (idempotent).
    RemoveReaction {
        channel: u64,
        message: u64,
        emoji: ReactionEmoji,
    },
    /// Site totals (admins only; every `Admin*` command answers `Unimplemented` from a server without the service).
    AdminOverview,
    /// Per-day activity of the last `days` days.
    AdminActivity {
        days: u32,
    },
    /// A page of servers. `epoch` names the list the answer belongs to (a new search starts a new one).
    AdminGuilds {
        epoch: u64,
        query: String,
        page_token: Option<String>,
        limit: u32,
    },
    AdminUsers {
        epoch: u64,
        query: String,
        page_token: Option<String>,
        limit: u32,
    },
    /// Stored files, largest first.
    AdminFiles {
        epoch: u64,
        guild: Option<u64>,
        kind: Option<AdminFileKind>,
        page_token: Option<String>,
        limit: u32,
    },
    AdminAudit {
        epoch: u64,
        page_token: Option<String>,
        limit: u32,
    },
    AdminSetAdmin {
        user: u64,
        is_admin: bool,
    },
    AdminDeleteFile {
        key: String,
        reason: String,
    },
    // ---- account lifecycle ------------------------------------------------------------------
    /// Whether the terms changed since the user accepted them: `Reply::Terms`.
    CheckTerms,
    /// Accepts the current terms: `Reply::TermsAccepted`.
    AcceptTerms {
        version: String,
    },
    /// Emails a deletion code (for accounts without a password): `Reply::DeletionCodeSent`.
    RequestAccountDeletionCode,
    /// Deletes the account (anonymized); the password, or the emailed code without one, plus a
    /// TOTP or recovery code when two-factor authentication is on: `Reply::AccountDeleted`.
    DeleteAccount {
        password: Option<String>,
        email_code: Option<String>,
        totp_code: Option<String>,
    },
    /// Emails a link to an export of the user's data: `Reply::DataExportQueued`.
    ExportData,
    /// Site admins: `Reply::AdminUserSuspended`.
    AdminSuspendUser {
        user: u64,
        reason: String,
        until: Option<std::time::SystemTime>,
    },
    /// Site admins: `Reply::AdminUserUnsuspended`.
    AdminUnsuspendUser {
        user: u64,
    },
    /// Site admins: `Reply::AdminUserDeleted`; `force_transfer` hands the user's guilds over.
    AdminDeleteUser {
        user: u64,
        force_transfer: bool,
        reason: String,
    },
    SignOut,
    /// Launch: validate the saved session, if there is one.
    Resume,
}

/// A signed-in session: the token the server issued and who it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub me: User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initial {
    pub me: User,
    pub guilds: Vec<Guild>,
    pub channels: Vec<Channel>,
    /// Each guild's section layout.
    pub layouts: HashMap<u64, GuildLayout>,
    pub dms: Vec<DmChannel>,
}

type R<T> = Result<T, ApiError>;

impl Command {
    /// Whether the command is answered without a session, by the server in the auth form's field.
    pub fn signed_out(&self) -> bool {
        matches!(
            self,
            Command::Login { .. }
                | Command::RequestEmailCode { .. }
                | Command::VerifyEmailCode { .. }
                | Command::CompleteSecondFactor { .. }
                | Command::BeginSecondFactorPasskey { .. }
                | Command::RequestPasswordReset { .. }
                | Command::ResetPassword { .. }
                | Command::BeginPasskeyLogin { .. }
                | Command::LookupInvite { .. }
                | Command::AcceptInvite { .. }
        )
    }
}

/// What this device is called in the account's session list (and a new passkey's default name):
/// the computer's name, else "thehideout desktop on <OS>".
pub fn device_label() -> String {
    let host = gethostname::gethostname().to_string_lossy().trim().trim_end_matches(".local").to_owned();
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    if host.is_empty() || host == "localhost" {
        format!("thehideout desktop on {os}")
    } else {
        host.chars().take(64).collect()
    }
}

/// The Security page's lists.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SecurityInfo {
    pub passkeys: Vec<Passkey>,
    pub sessions: Vec<DeviceSession>,
    /// Whether an authenticator app is on, recovery codes left, a password, a confirmed email.
    pub summary: SecuritySummary,
}

/// What a `RevokeInvite` / `ResendInvite` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteAction {
    Revoke,
    Resend,
}

/// A guild's roles and the role ids each member holds.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GuildAccess {
    pub roles: Vec<Role>,
    /// The roles' version, for `Command::ReorderRoles`.
    pub version: u64,
    pub member_roles: HashMap<u64, Vec<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// A sign-in finished: `Login`, `VerifyEmailCode`, `CompleteSecondFactor`, `AcceptInvite` or
    /// a passkey. Its error belongs to whatever step the form is on.
    Auth(R<Session>),
    /// `Login` / `VerifyEmailCode` for an account with an authenticator app: answer with
    /// `CompleteSecondFactor`.
    SecondFactor {
        challenge: String,
        methods: Vec<SecondFactorMethod>,
    },
    /// Answer to `RequestEmailCode` and `RequestPasswordReset`.
    CodeSent(R<()>),
    PasswordReset(R<()>),
    /// A passkey ceremony page to open in the browser (or why it couldn't start).
    PasskeyCeremony(R<String>),
    InviteLookup(R<InvitePreview>),
    Security(R<SecurityInfo>),
    PasswordChanged(R<()>),
    /// Answer to `ChangeEmail`: the code is on its way.
    EmailChangeStarted(R<()>),
    EmailChanged(R<User>),
    TotpSetup(R<TotpSetup>),
    /// Answer to `ConfirmTotp` and (`regenerated`) `RegenerateRecoveryCodes`.
    RecoveryCodes {
        regenerated: bool,
        result: R<Vec<String>>,
    },
    TotpDisabled(R<()>),
    PasskeyAdded(R<Passkey>),
    PasskeyRenamed(R<Passkey>),
    PasskeyDeleted {
        id: u64,
        result: R<()>,
    },
    SessionRevoked {
        id: u64,
        result: R<()>,
    },
    /// How many sessions `RevokeOtherSessions` signed out.
    OthersRevoked(R<u32>),
    InviteCreated(R<Invite>),
    Invites {
        epoch: u64,
        guild: Option<u64>,
        result: R<Paged<Invite>>,
    },
    InviteSaved {
        id: u64,
        action: InviteAction,
        result: R<Invite>,
    },
    Initial(R<Initial>),
    /// Answer to `Refresh`.
    Refreshed(R<Initial>),
    Channels {
        guild: u64,
        result: R<(Vec<Channel>, GuildLayout)>,
    },
    Access {
        guild: u64,
        result: R<GuildAccess>,
    },
    /// Answer to `CreateSection` and `RenameSection`.
    SectionSaved {
        guild: u64,
        result: R<(Section, GuildLayout)>,
    },
    SectionDeleted {
        guild: u64,
        result: R<GuildLayout>,
    },
    LayoutSaved {
        guild: u64,
        result: R<GuildLayout>,
    },
    ChannelSaved {
        guild: u64,
        result: R<Channel>,
    },
    Messages {
        channel: u64,
        before: Option<u64>,
        result: R<Page<Message>>,
    },
    Sent {
        channel: u64,
        nonce: String,
        result: R<Message>,
    },
    /// An upload moved: `sent` of `total` bytes are on their way.
    UploadProgress {
        id: u64,
        sent: u64,
        total: u64,
    },
    /// The bytes of a picked picture, for its chip's thumbnail (read off the UI thread).
    UploadPreview {
        id: u64,
        bytes: Vec<u8>,
    },
    /// Answer to `UploadFile` and `UploadBytes`.
    Uploaded {
        id: u64,
        result: R<PendingAttachment>,
    },
    DownloadProgress {
        key: String,
        received: u64,
        total: Option<u64>,
    },
    /// Answer to `Download`: the bytes written.
    Downloaded {
        key: String,
        path: PathBuf,
        result: R<u64>,
    },
    GuildCreated(R<Guild>),
    GuildJoined(R<Guild>),
    ChannelCreated(R<Channel>),
    /// Answer to `JoinVoice`: what to connect to.
    VoiceJoined {
        channel: u64,
        result: R<VoiceJoin>,
    },
    /// Answer to `UpdateVoiceState`.
    VoiceFlags(R<VoiceState>),
    /// Answer to `LeaveVoice`.
    VoiceLeft(R<()>),
    /// Answer to `ServerMuteMember`, `MoveMember` and `DisconnectMember` (the new state itself
    /// arrives as a gateway event).
    VoiceModerated {
        guild: u64,
        user: u64,
        result: R<()>,
    },
    Members {
        guild: u64,
        result: R<Vec<User>>,
    },
    /// Answer to `EditMessage`: the message as stored.
    MessageEdited {
        channel: u64,
        message: u64,
        result: R<Message>,
    },
    MessageDeleted {
        channel: u64,
        message: u64,
        result: R<()>,
    },
    Kicked {
        guild: u64,
        user: u64,
        result: R<()>,
    },
    Banned {
        guild: u64,
        user: u64,
        result: R<()>,
    },
    Unbanned {
        guild: u64,
        user: u64,
        result: R<()>,
    },
    Bans {
        guild: u64,
        result: R<Vec<Ban>>,
    },
    GuildLeft {
        guild: u64,
        result: R<()>,
    },
    /// Answer to `DeleteChannel`: the new layout.
    ChannelDeleted {
        guild: u64,
        channel: u64,
        result: R<GuildLayout>,
    },
    GuildDeleted {
        guild: u64,
        result: R<()>,
    },
    InviteRotated {
        guild: u64,
        result: R<String>,
    },
    OwnershipTransferred {
        guild: u64,
        user: u64,
        result: R<Guild>,
    },
    DmOpened(R<DmChannel>),
    /// Answer to `FetchMedia`.
    Media {
        key: String,
        result: R<Vec<u8>>,
    },
    /// Answer to `ReadImage`: the bytes and their content type.
    ImageRead {
        purpose: ImagePurpose,
        result: R<(Vec<u8>, String)>,
    },
    /// Answer to `UploadAvatar` and `RemoveAvatar`.
    AvatarSaved(R<User>),
    /// Answer to `UploadGuildIcon`, `RemoveGuildIcon` and `RenameGuild`.
    GuildSaved {
        guild: u64,
        result: R<Guild>,
    },
    RoleCreated {
        guild: u64,
        result: R<(Role, u64)>,
    },
    RoleUpdated {
        guild: u64,
        result: R<Role>,
    },
    RoleDeleted {
        guild: u64,
        role: u64,
        result: R<u64>,
    },
    RolesReordered {
        guild: u64,
        result: R<RoleList>,
    },
    MemberRolesSet {
        guild: u64,
        user: u64,
        result: R<Member>,
    },
    /// Answer to `LoadEmojis`.
    Emojis {
        guild: u64,
        result: R<Vec<Emoji>>,
    },
    /// Answer to `CreateEmoji` and `RenameEmoji`.
    EmojiSaved {
        guild: u64,
        result: R<Emoji>,
    },
    EmojiDeleted {
        guild: u64,
        emoji: u64,
        result: R<()>,
    },
    /// Answer to `AddReaction` (`add`) and `RemoveReaction`.
    ReactionSaved {
        channel: u64,
        message: u64,
        emoji: ReactionEmoji,
        add: bool,
        result: R<()>,
    },
    AdminOverview(R<Overview>),
    AdminActivity {
        days: u32,
        result: R<Vec<ActivityPoint>>,
    },
    AdminGuilds {
        epoch: u64,
        result: R<Paged<AdminGuildRow>>,
    },
    AdminUsers {
        epoch: u64,
        result: R<Paged<AdminUserRow>>,
    },
    AdminFiles {
        epoch: u64,
        result: R<Paged<AdminFileRow>>,
    },
    AdminAudit {
        epoch: u64,
        result: R<Paged<AuditEntry>>,
    },
    AdminAdminSet {
        user: u64,
        is_admin: bool,
        result: R<User>,
    },
    AdminFileDeleted {
        key: String,
        result: R<()>,
    },
    /// Answer to `CheckTerms`.
    Terms(R<TermsStatus>),
    TermsAccepted {
        version: String,
        result: R<()>,
    },
    DeletionCodeSent(R<()>),
    /// The account is gone and its session dead.
    AccountDeleted(R<()>),
    /// The address the export link goes to, masked.
    DataExportQueued(R<String>),
    AdminUserSuspended {
        user: u64,
        result: R<AdminUserRow>,
    },
    AdminUserUnsuspended {
        user: u64,
        result: R<AdminUserRow>,
    },
    AdminUserDeleted {
        user: u64,
        result: R<()>,
    },
    SignedOut,
    /// Answer to `Resume`: no saved session, a valid one (its server URL and the session), or why
    /// it couldn't be used (`Unauthenticated`: the token was refused and is forgotten).
    Resume(R<Option<(String, Session)>>),
    Gateway(GatewayUpdate),
}

pub trait Backend {
    fn submit(&mut self, cmd: Command);
    /// Replies that are due at `now` (the app clock, seconds). Called from `poll`.
    fn poll(&mut self, now: f64) -> Vec<Reply>;
    /// When the next queued reply is due, so the app can schedule a poll.
    fn next_due(&self) -> Option<f64> {
        None
    }
    /// Where a stored file lives on this machine, when it does (the fake world's clips): a video
    /// plays from there instead of from the server's URL.
    fn media_path(&self, _key: &str) -> Option<PathBuf> {
        None
    }
    /// Called once with the shell's waker: wake it whenever a reply becomes available.
    fn set_waker(&mut self, _waker: rune::Waker) {}
    /// The server the next sign-in talks to (the auth form's field; empty: the default).
    fn set_server(&mut self, _url: &str) {}
    /// Every command submitted so far (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    fn sent(&self) -> Vec<Command> {
        Vec::new()
    }
}
