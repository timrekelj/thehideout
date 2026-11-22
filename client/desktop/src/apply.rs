//! Replies and gateway events become state here. `apply` is the only way data enters
//! `AppState`: pure transitions plus the few `Tx` hints (which commits animate), so the dedupe
//! and pagination rules are unit-tested without a window.

use std::time::Duration;

use rune::Tx;
use thehideout_api::{
    ApiError, Channel, ChannelKind, DmChannel, Event, GatewayUpdate, Guild, Message, Page, User,
};
use thehideout_design::components::{ConnectionStatus, ToastKind};

use crate::account::SecReply;
use crate::backend::{Command, GuildAccess, Initial, Reply, Session};
use crate::layout;
use crate::server_settings::{ROLES_CONFLICT_TOAST, RoleForm, error_text};
use crate::state::{
    AppState, Delivery, DialogKind, Fetch, Marker, Phase, Place, Screen, Stream, auth_error,
    chat_id_of, sentence,
};

pub fn apply(s: &mut AppState, reply: Reply, tx: &mut Tx) {
    // A picture arriving can't bring news of other pictures; everything else may.
    let fetched = matches!(
        reply,
        Reply::Media { .. } | Reply::UploadProgress { .. } | Reply::DownloadProgress { .. }
    );
    apply_reply(s, reply, tx);
    if !fetched {
        s.want_media();
    }
}

fn apply_reply(s: &mut AppState, reply: Reply, tx: &mut Tx) {
    match reply {
        Reply::Auth(Ok(session)) => {
            s.auth_succeeded(&session);
            signed_in(s, session)
        }
        // A refused sign-in stays on its step with what was typed (a resumed or loading session
        // that fails resets below instead).
        Reply::Auth(Err(e)) => s.auth_refused(&e),
        Reply::SecondFactor { challenge, methods } => s.second_factor(challenge, methods),
        Reply::CodeSent(r) => s.code_sent(r),
        Reply::PasswordReset(r) => s.password_reset(r),
        Reply::PasskeyCeremony(r) => s.passkey_ceremony(r, tx),
        Reply::InviteLookup(r) => s.invite_looked_up(r),
        Reply::Security(r) => s.security_loaded(r),
        Reply::PasswordChanged(r) => s.sec_replied(SecReply::PasswordChanged(r)),
        Reply::EmailChangeStarted(r) => s.sec_replied(SecReply::EmailChangeStarted(r)),
        Reply::EmailChanged(r) => s.sec_replied(SecReply::EmailChanged(r)),
        Reply::TotpSetup(r) => s.sec_replied(SecReply::TotpSetup(r)),
        Reply::RecoveryCodes { regenerated, result } => {
            s.sec_replied(SecReply::RecoveryCodes { regenerated, result })
        }
        Reply::TotpDisabled(r) => s.sec_replied(SecReply::TotpDisabled(r)),
        Reply::PasskeyAdded(r) => s.sec_replied(SecReply::PasskeyAdded(r)),
        Reply::PasskeyRenamed(r) => s.sec_replied(SecReply::PasskeyRenamed(r)),
        Reply::PasskeyDeleted { id, result } => s.sec_replied(SecReply::PasskeyDeleted { id, result }),
        Reply::SessionRevoked { id, result } => s.sec_replied(SecReply::SessionRevoked { id, result }),
        Reply::OthersRevoked(r) => s.sec_replied(SecReply::OthersRevoked(r)),
        Reply::InviteCreated(r) => s.invite_created(r),
        Reply::Invites { epoch, guild, result } => s.invites_listed(epoch, guild, result),
        Reply::InviteSaved { id, action, result } => s.invite_saved(id, action, result),
        Reply::Initial(Ok(init)) => {
            initial(s, init);
            // Auth fades into the shell; everything inside it rides that one entrance.
            tx.animated();
        }
        // A resumed session that is refused leaves the form without a complaint: the user typed
        // nothing wrong. (An interactive sign-in has the form busy.)
        Reply::Initial(Err(ApiError::Unauthenticated)) if s.auth.phase != Phase::Busy => {
            s.reset(None)
        }
        Reply::Initial(Err(e)) => auth_failed(s, &e),
        Reply::Resume(Ok(None)) | Reply::Resume(Err(ApiError::Unauthenticated)) => s.reset(None),
        Reply::Resume(Ok(Some((server, session)))) => {
            s.auth.server.replace_text(&server);
            signed_in(s, session);
        }
        Reply::Resume(Err(e)) => auth_failed(s, &e),
        Reply::Refreshed(Ok(init)) => refreshed(s, init),
        // Quiet: the lists stay as they were and the next gap tries again.
        Reply::Refreshed(Err(_)) => {}
        Reply::Channels { guild, result } => match result {
            Ok((list, layout)) => {
                set_channels(s, guild, list);
                // An optimistic edit still out is newer than this answer; its own reply settles it.
                if !s.data.layout_undo.contains_key(&guild) {
                    s.data.layouts.insert(guild, layout::normalized(&layout));
                }
            }
            Err(e) => s.toast(
                ToastKind::Danger,
                format!("Couldn't load channels. {}", sentence(&e.to_string())),
            ),
        },
        Reply::Access { guild, result } => match result {
            Ok(GuildAccess {
                roles,
                version,
                member_roles,
            }) => {
                s.set_roles(guild, roles, Some(version));
                s.data.member_roles.insert(guild, member_roles);
            }
            Err(_) => {
                s.data.access_asked.remove(&guild);
            }
        },
        Reply::SectionSaved { guild, result } => match result {
            Ok((section, layout)) => {
                s.set_layout(guild, layout);
                let kind = s.ui.dialog.as_ref().map(|d| d.kind);
                if matches!(kind, Some(DialogKind::CreateSection { guild: g }) | Some(DialogKind::RenameSection { guild: g, .. }) if g == guild)
                {
                    s.close_dialog();
                }
                s.toast(ToastKind::Success, format!("Saved {}", section.name));
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::SectionDeleted { guild, result } => match result {
            Ok(layout) => {
                s.set_layout(guild, layout);
                s.close_dialog();
                s.toast(ToastKind::Success, "Section deleted");
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::LayoutSaved { guild, result } => s.layout_replied(guild, result),
        Reply::ChannelSaved { guild: _, result } => match result {
            Ok(c) => {
                update_channel(s, c);
                s.close_dialog();
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::Messages {
            channel,
            before,
            result,
        } => messages(s, channel, before, result, tx),
        Reply::Sent {
            channel,
            nonce,
            result,
        } => sent(s, channel, &nonce, result),
        Reply::UploadProgress { id, sent, total } => s.upload_progress(id, sent, total),
        Reply::UploadPreview { id, bytes } => s.upload_preview(id, bytes),
        Reply::Uploaded { id, result } => s.uploaded(id, result),
        Reply::DownloadProgress {
            key,
            received,
            total,
        } => s.download_progress(&key, received, total),
        Reply::Downloaded { key, path, result } => s.downloaded(&key, &path, result),
        Reply::GuildCreated(r) => guild_done(s, r, DialogKind::CreateGuild, "Server created"),
        Reply::GuildJoined(r) => guild_done(s, r, DialogKind::JoinGuild, "Joined the server"),
        Reply::ChannelCreated(r) => match r {
            Ok(c) => {
                let guild = c.guild_id;
                let name = c.name.clone();
                add_channel(s, c.clone());
                // The server's layout update follows on the gateway; this keeps the row from waiting for it.
                if let Some(l) = s.data.layouts.get(&guild) {
                    let with = layout::with_channel(l, c.id, c.section_id);
                    s.data.layouts.insert(guild, with);
                }
                s.close_dialog();
                // A new voice channel is not joined on the creator's behalf.
                if s.ui.place == Place::Guild(guild) && c.kind == ChannelKind::Text {
                    s.select_channel(c.id);
                }
                s.toast(ToastKind::Success, format!("Created #{name}"));
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::VoiceJoined { channel, result } => s.voice_joined(channel, result),
        Reply::VoiceFlags(r) => match r {
            // The new state also arrives as an event; a late reply must not undo a later change.
            Ok(_) => {}
            Err(e) => s.voice_failed(&e),
        },
        Reply::VoiceLeft(r) | Reply::VoiceModerated { result: r, .. } => {
            if let Err(e) = r {
                s.voice_failed(&e);
            }
        }
        Reply::Members { guild, result } => match result {
            Ok(users) => {
                let ids = users.iter().map(|u| u.id).collect();
                for u in users {
                    s.data.users.insert(u.id, u);
                }
                s.data.members.insert(guild, ids);
            }
            // Asked again the next time the guild is opened or a stranger speaks.
            Err(_) => {
                s.data.members_asked.remove(&guild);
            }
        },
        Reply::DmOpened(r) => match r {
            Ok(dm) => {
                let id = dm.id;
                let peer = s
                    .dm_peer(&dm)
                    .map(|u| u.username.clone())
                    .unwrap_or_default();
                add_dm(s, dm);
                s.close_dialog();
                s.go_home();
                s.select_channel(id);
                s.toast(
                    ToastKind::Success,
                    format!("Opened a conversation with {peer}"),
                );
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::Media { key, result } => match result {
            Ok(bytes) if s.is_emoji_image(&key) => s.data.media.insert_emoji(&key, bytes),
            Ok(bytes) => s.data.media.insert(&key, bytes),
            Err(_) => s.data.media.fail(&key),
        },
        Reply::ImageRead { purpose, result } => s.image_read(purpose, result),
        Reply::AvatarSaved(r) => match r {
            Ok(u) => {
                let had_draft =
                    s.ui.dialog
                        .as_ref()
                        .is_some_and(|d| d.kind == DialogKind::Profile && d.pick.draft.is_some());
                user_updated(s, u);
                if s.ui
                    .dialog
                    .as_ref()
                    .is_some_and(|d| d.kind == DialogKind::Profile)
                {
                    s.close_dialog();
                }
                s.toast(
                    ToastKind::Success,
                    if had_draft {
                        "Profile picture updated"
                    } else {
                        "Profile picture removed"
                    },
                );
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::GuildSaved { guild, result } => guild_saved(s, guild, result),
        Reply::RoleCreated { guild, result } => match result {
            Ok((role, version)) => {
                let fresh = s.role(guild, role.id).is_none();
                let mut roles = s.roles_of(guild).to_vec();
                if fresh {
                    // The server puts it at the bottom and moves everyone up one.
                    roles.iter_mut().for_each(|r| r.position += 1);
                    roles.push(role.clone());
                }
                s.set_roles(guild, roles, Some(version));
                let name = role.name.clone();
                if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild)
                    && ss.form.as_ref().is_some_and(|f| f.role.is_none())
                {
                    ss.form = Some(RoleForm::of(&role));
                }
                s.toast(ToastKind::Success, format!("Created {name}"));
            }
            Err(e) => role_form_failed(s, &e),
        },
        Reply::RoleUpdated { guild, result } => match result {
            Ok(role) => {
                let mut roles = s.roles_of(guild).to_vec();
                if let Some(slot) = roles.iter_mut().find(|r| r.id == role.id) {
                    *slot = role.clone();
                }
                s.set_roles(guild, roles, None);
                if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild)
                    && ss.form.as_ref().is_some_and(|f| f.role == Some(role.id))
                {
                    ss.form = Some(RoleForm::of(&role));
                }
                s.toast(ToastKind::Success, format!("Saved {}", role.name));
            }
            Err(e) => role_form_failed(s, &e),
        },
        Reply::RoleDeleted {
            guild,
            role,
            result,
        } => match result {
            Ok(version) => {
                let roles: Vec<_> = s
                    .roles_of(guild)
                    .iter()
                    .filter(|r| r.id != role)
                    .cloned()
                    .collect();
                s.set_roles(guild, roles, Some(version));
                if let Some(m) = s.data.member_roles.get_mut(&guild) {
                    m.values_mut().for_each(|ids| ids.retain(|r| *r != role));
                }
                if matches!(s.ui.dialog.as_ref().map(|d| d.kind), Some(DialogKind::DeleteRole { role: r, .. }) if r == role)
                {
                    s.close_dialog();
                }
                s.toast(ToastKind::Success, "Role deleted");
            }
            Err(e) => dialog_failed(s, &e),
        },
        Reply::RolesReordered { guild, result } => {
            let undo = s.data.role_undo.remove(&guild);
            match result {
                Ok(list) => s.set_roles(guild, list.roles, Some(list.version)),
                // Someone else changed the roles first: show what is true now, and say so.
                Err(ApiError::Conflict) => {
                    s.outbox.push(Command::LoadAccess { guild });
                    s.toast(ToastKind::Danger, ROLES_CONFLICT_TOAST);
                }
                Err(e) => {
                    if let Some(old) = undo {
                        s.set_roles(guild, old, None);
                    }
                    if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild) {
                        ss.role_error = Some(error_text(&e));
                    }
                }
            }
        }
        Reply::MemberRolesSet {
            guild,
            user,
            result,
        } => {
            if let Some(ss) =
                s.ui.server_settings
                    .as_mut()
                    .filter(|x| x.guild == guild && x.member_busy == Some(user))
            {
                ss.member_busy = None;
            }
            match result {
                Ok(member) => {
                    s.data
                        .member_roles
                        .entry(guild)
                        .or_default()
                        .insert(member.user.id, member.role_ids);
                    s.data.users.insert(member.user.id, member.user);
                }
                Err(e) => {
                    if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild) {
                        ss.member_error = Some(error_text(&e));
                    }
                }
            }
        }
        Reply::Emojis { guild, result } => match result {
            Ok(list) => s.set_emojis(guild, list),
            // Asked again the next time the server is opened.
            Err(_) => {
                s.data.emojis_asked.remove(&guild);
            }
        },
        Reply::EmojiSaved { guild, result } => s.emoji_saved(guild, result),
        Reply::EmojiDeleted {
            guild,
            emoji,
            result,
        } => s.emoji_deleted(guild, emoji, result),
        Reply::ReactionSaved {
            channel,
            message,
            emoji,
            add,
            result,
        } => s.reaction_saved(channel, message, &emoji, add, result),
        Reply::AdminOverview(r) => s.admin_overview_replied(r),
        Reply::AdminActivity { days, result } => s.admin_activity_replied(days, result),
        Reply::AdminGuilds { epoch, result } => s.admin_guilds_replied(epoch, result),
        Reply::AdminUsers { epoch, result } => s.admin_users_replied(epoch, result),
        Reply::AdminFiles { epoch, result } => s.admin_files_replied(epoch, result),
        Reply::AdminAudit { epoch, result } => s.admin_audit_replied(epoch, result),
        Reply::AdminAdminSet {
            user,
            is_admin,
            result,
        } => s.admin_set_replied(user, is_admin, result),
        Reply::AdminFileDeleted { key, result } => s.admin_file_deleted(&key, result),
        Reply::SignedOut => {}
        Reply::Gateway(update) => gateway(s, update, tx),
    }
}

/// A user's profile changed (their avatar): every copy of them is replaced, and a new picture fetched.
fn user_updated(s: &mut AppState, u: User) {
    if let Some(session) = s.session.as_mut().filter(|x| x.me.id == u.id) {
        session.me = u.clone();
    }
    for dm in &mut s.data.dms {
        for p in dm.participants.iter_mut().filter(|p| p.id == u.id) {
            *p = u.clone();
        }
    }
    s.data.users.insert(u.id, u);
    s.want_media();
}

fn guild_saved(s: &mut AppState, guild: u64, result: Result<Guild, ApiError>) {
    let name_busy =
        s.ui.server_settings
            .as_ref()
            .is_some_and(|x| x.guild == guild && x.name_busy);
    match result {
        Ok(g) => {
            s.set_guild(g);
            if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild) {
                ss.name_busy = false;
                ss.icon_busy = false;
                ss.icon.draft = None;
            }
            s.toast(
                ToastKind::Success,
                if name_busy {
                    "Server renamed"
                } else {
                    "Server icon updated"
                },
            );
        }
        Err(e) => {
            if let Some(ss) = s.ui.server_settings.as_mut().filter(|x| x.guild == guild) {
                if name_busy {
                    ss.name_error = Some(error_text(&e));
                } else {
                    ss.icon_error = Some(error_text(&e));
                }
                ss.name_busy = false;
                ss.icon_busy = false;
            }
        }
    }
}

/// A role save the server refused: under the editor's fields, where the user is looking.
fn role_form_failed(s: &mut AppState, e: &ApiError) {
    if let Some(f) = s.role_form() {
        f.busy = false;
        f.error = Some(error_text(e));
    }
}

fn signed_in(s: &mut AppState, session: Session) {
    s.data.users.insert(session.me.id, session.me.clone());
    s.session = Some(session);
    // Stay on the form (busy) until the shell has its data, so it never shows empty.
    s.outbox.push(Command::LoadInitial);
}

fn auth_failed(s: &mut AppState, e: &ApiError) {
    s.reset(Some(auth_error(e)));
}

fn initial(s: &mut AppState, init: Initial) {
    let Initial {
        me,
        guilds,
        channels,
        layouts,
        dms,
    } = init;
    s.data.layouts = layouts
        .iter()
        .map(|(g, l)| (*g, layout::normalized(l)))
        .collect();
    if let Some(session) = &mut s.session {
        session.me = me.clone();
    }
    s.data.users.insert(me.id, me);
    s.data.guilds = guilds;
    s.data.channels.clear();
    for g in &s.data.guilds {
        s.data.channels.insert(g.id, Vec::new());
    }
    for c in channels {
        s.data.channels.entry(c.guild_id).or_default().push(c);
    }
    for dm in &dms {
        for u in &dm.participants {
            s.data.users.insert(u.id, u.clone());
        }
    }
    s.data.dms = dms;
    s.screen = Screen::Main;
    s.auth.phase = Phase::Idle;
    s.auth.password.clear();
    s.ui.place = Place::Home;
    match s.data.guilds.first().map(|g| g.id) {
        Some(g) => s.go_guild(g),
        None => s.go_home(),
    }
    s.after_initial();
}

/// The lists again after a gateway gap. Unlike `initial`, the place, the selection and the
/// loaded history stay as they are.
fn refreshed(s: &mut AppState, init: Initial) {
    if s.screen != Screen::Main {
        return;
    }
    let Initial {
        me,
        guilds,
        channels,
        layouts,
        dms,
    } = init;
    for (g, l) in layouts {
        // An edit still out is newer than this answer; its reply settles the layout.
        if !s.data.layout_undo.contains_key(&g) {
            s.data.layouts.insert(g, layout::normalized(&l));
        }
    }
    // Roles and assignments may have changed while away: the open server asks again below, the others when visited.
    s.data.roles.clear();
    s.data.role_undo.clear();
    s.data.member_roles.clear();
    s.data.access_asked.clear();
    s.data.users.insert(me.id, me);
    for dm in &dms {
        for u in &dm.participants {
            s.data.users.insert(u.id, u.clone());
        }
    }
    s.data.channels.clear();
    for g in &guilds {
        s.data.channels.insert(g.id, Vec::new());
    }
    for c in channels {
        s.data.channels.entry(c.guild_id).or_default().push(c);
    }
    s.data.guilds = guilds;
    s.data.dms = dms;
    // Members who joined while away: the open server asks now, the others when visited.
    let open = match s.ui.place {
        Place::Guild(g) => Some(g),
        Place::Home => None,
    };
    s.data.members.retain(|g, _| Some(*g) == open);
    s.data.members_asked.retain(|g| Some(*g) == open);
    // Emoji changes missed while away: the open server's list is fetched again now (what is shown
    // stays until it lands), the others when visited.
    s.data.emojis.retain(|g, _| Some(*g) == open);
    s.data.emojis_asked.retain(|g| Some(*g) == open);
    match open {
        Some(g) if s.guild(g).is_none() => s.go_home(),
        Some(g) => {
            if s.current_channel().is_none()
                && let Some(first) = s.first_text_channel(g)
            {
                s.select_channel(first);
            }
            s.outbox.push(Command::LoadMembers { guild: g });
            s.data.members_asked.insert(g);
            s.data.emojis_asked.insert(g);
            s.outbox.push(Command::LoadEmojis { guild: g });
            s.want_access(g);
        }
        None => {}
    }
}

fn set_channels(s: &mut AppState, guild: u64, list: Vec<Channel>) {
    s.data.channels.insert(guild, list);
    if s.ui.place == Place::Guild(guild)
        && s.current_channel().is_none()
        && let Some(first) = s.first_text_channel(guild)
    {
        s.select_channel(first);
    }
}

fn update_channel(s: &mut AppState, c: Channel) {
    if let Some(slot) = s
        .data
        .channels
        .get_mut(&c.guild_id)
        .and_then(|l| l.iter_mut().find(|x| x.id == c.id))
    {
        *slot = c;
    }
}

fn add_channel(s: &mut AppState, c: Channel) {
    let list = s.data.channels.entry(c.guild_id).or_default();
    if !list.iter().any(|x| x.id == c.id) {
        list.push(c);
    }
}

fn add_dm(s: &mut AppState, dm: DmChannel) {
    for u in &dm.participants {
        s.data.users.insert(u.id, u.clone());
    }
    if !s.data.dms.iter().any(|d| d.id == dm.id) {
        s.data.dms.push(dm);
    }
}

fn add_guild(s: &mut AppState, g: Guild) {
    if !s.data.guilds.iter().any(|x| x.id == g.id) {
        s.data.guilds.push(g);
    }
}

fn guild_done(s: &mut AppState, r: Result<Guild, ApiError>, kind: DialogKind, message: &str) {
    match r {
        Ok(g) => {
            let id = g.id;
            add_guild(s, g);
            if matches!(s.ui.dialog.as_ref().map(|d| d.kind), Some(k) if k == kind) {
                s.close_dialog();
            }
            s.go_guild(id);
            s.toast(ToastKind::Success, message);
        }
        Err(e) => dialog_failed(s, &e),
    }
}

fn dialog_failed(s: &mut AppState, e: &ApiError) {
    let message = match e {
        ApiError::NotFound => "That invite code doesn't match any server.".to_owned(),
        ApiError::AlreadyExists
            if matches!(
                s.ui.dialog.as_ref().map(|d| d.kind),
                Some(DialogKind::JoinGuild)
            ) =>
        {
            "You're already in that server.".to_owned()
        }
        ApiError::AlreadyExists => "That name is already taken here.".to_owned(),
        ApiError::PermissionDenied => "You don't have permission to do that.".to_owned(),
        ApiError::Conflict => {
            "That changed while you were editing. Close this and try again.".to_owned()
        }
        ApiError::InvalidArgument(m) => sentence(m),
        ApiError::Network(_) => "Can't reach the server.".to_owned(),
        other => sentence(&other.to_string()),
    };
    match &mut s.ui.dialog {
        Some(d) => {
            d.phase = Phase::Idle;
            d.error = Some(message);
        }
        None => s.toast(ToastKind::Danger, message),
    }
}

fn messages(
    s: &mut AppState,
    channel: u64,
    before: Option<u64>,
    result: Result<Page<Message>, ApiError>,
    tx: &mut Tx,
) {
    let me = s.my_id();
    let Some(log) = s.data.logs.get_mut(&channel) else {
        return;
    };
    let page = match result {
        Ok(p) => p,
        Err(e) => {
            // A failed refresh of what is shown is tried again on the next visit.
            log.stale = before.is_none() && log.loaded;
            if before.is_some() || log.fetch != Fetch::Older {
                log.fetch = Fetch::Idle;
            }
            s.toast(
                ToastKind::Danger,
                format!("Couldn't load messages. {}", sentence(&e.to_string())),
            );
            return;
        }
    };
    // Lands at once: a page arriving is data, not a reaction to the reader.
    tx.instant();
    // A refresh of the newest page doesn't end an older page's fetch that is still out.
    if before.is_some() || log.fetch != Fetch::Older {
        log.fetch = Fetch::Idle;
    }
    match before {
        Some(_) => {
            let first = log.messages.first().cloned();
            if let Some(f) = &first {
                log.breaks.push(f.id);
            }
            log.insert_page(page.items);
            log.has_more = page.has_more;
            if let Some(first_msg) = &first {
                // Hold the row the reader was looking at while rows are added above it.
                tx.keep_on_screen(format!("msg-{}", chat_id_of(first_msg, me)));
            }
        }
        None => {
            let newest_page_oldest = page.items.iter().map(|m| m.id).min();
            let gap = log.loaded
                && matches!((newest_page_oldest, log.newest_id()), (Some(o), Some(n)) if o > n);
            if !log.loaded || gap {
                log.messages.clear();
                log.has_more = page.has_more;
            }
            log.insert_page(page.items);
            log.loaded = true;
            log.stale = false;
        }
    }
    log.oldest_id = log.messages.first().map(|m| m.id);
    let strangers: Vec<Message> = log
        .messages
        .iter()
        .filter(|m| !s.data.users.contains_key(&m.author_id))
        .cloned()
        .collect();
    for m in &strangers {
        s.want_author(m);
    }
    resolve_marker(s, channel);
}

/// A "New" divider that was waiting for its history gets its place.
fn resolve_marker(s: &mut AppState, channel: u64) {
    let Marker::Pending { channel: c, unread } = s.ui.marker else {
        return;
    };
    if c != channel {
        return;
    }
    if let Some(log) = s.data.logs.get(&channel) {
        let at = log.messages.len().checked_sub(unread as usize + 1);
        s.ui.marker = Marker::After {
            channel,
            id: at.map(|i| log.messages[i].id),
        };
    }
}

fn sent(s: &mut AppState, channel: u64, nonce: &str, result: Result<Message, ApiError>) {
    match result {
        Ok(m) => {
            s.data.outgoing.retain(|o| o.nonce != nonce);
            if let Some(log) = s.data.logs.get_mut(&channel).filter(|l| l.loaded) {
                log.insert(m);
            }
        }
        Err(e) => {
            if let Some(o) = s.data.outgoing.iter_mut().find(|o| o.nonce == nonce) {
                o.state = Delivery::Failed;
            }
            if !e.is_transient() {
                s.toast(
                    ToastKind::Danger,
                    format!("Message not sent. {}", sentence(&e.to_string())),
                );
            }
        }
    }
}

fn gateway(s: &mut AppState, update: GatewayUpdate, tx: &mut Tx) {
    match update {
        GatewayUpdate::Connecting => {
            if s.ui.connection != ConnectionStatus::Connected {
                s.ui.connection = ConnectionStatus::Connecting;
            }
        }
        GatewayUpdate::Connected => {
            s.ui.connection = ConnectionStatus::Connected;
            // A new stream starts with no voice states; a snapshot follows when anyone is in voice.
            s.clear_voice();
            // Events missed while disconnected aren't replayed: fetch them again.
            if std::mem::replace(&mut s.ui.stream, Stream::Live) == Stream::Interrupted {
                s.resync();
            }
        }
        GatewayUpdate::Disconnected { retry_in, .. } => {
            s.ui.stream = Stream::Interrupted;
            s.ui.connection = ConnectionStatus::Reconnecting(
                retry_in.max(Duration::from_secs(1)).as_secs() as u32,
            );
        }
        GatewayUpdate::AuthExpired => s.reset(Some(auth_error(&ApiError::Unauthenticated))),
        GatewayUpdate::Event(e) => event(s, e, tx),
    }
}

fn event(s: &mut AppState, e: Event, tx: &mut Tx) {
    match e {
        Event::Heartbeat => {}
        Event::EmojisUpdated { guild_id, emojis } => s.set_emojis(guild_id, emojis),
        Event::ReactionAdded {
            channel_id,
            message_id,
            emoji,
            user_id,
        } => s.reaction_event(channel_id, message_id, &emoji, user_id, true),
        Event::ReactionRemoved {
            channel_id,
            message_id,
            emoji,
            user_id,
        } => s.reaction_event(channel_id, message_id, &emoji, user_id, false),
        Event::MessageUpdated(m) => {
            if message_updated(s, m) {
                // Embeds arrive a moment after the message. At the end they ease in under it;
                // while the reader is up in older messages the page changes at once, so their
                // row holds its place (the transcript's scroll anchor does the rest).
                if s.ui.at_bottom {
                    tx.animated();
                } else {
                    tx.instant();
                }
            }
        }
        Event::GuildUpdated(g) => s.set_guild(g),
        // Joined or created elsewhere (another device): list it and load its channels and
        // layout, leaving the current place alone.
        Event::GuildAdded(g) => {
            let id = g.id;
            add_guild(s, g);
            if !s.data.channels.contains_key(&id) {
                s.outbox.push(Command::LoadChannels { guild: id });
            }
        }
        Event::ChannelUpdated(c) => update_channel(s, c),
        Event::UserUpdated(u) => user_updated(s, u),
        Event::LayoutUpdated { guild_id, layout } => s.set_layout(guild_id, layout),
        Event::RolesUpdated {
            guild_id,
            roles,
            version,
        } => s.set_roles(guild_id, roles, Some(version)),
        Event::MemberUpdated { guild_id, member } => {
            if let Some(m) = s.data.member_roles.get_mut(&guild_id) {
                m.insert(member.user.id, member.role_ids.clone());
            }
            s.data.users.insert(member.user.id, member.user);
        }
        Event::VoiceStateUpdate {
            guild_id,
            user_id,
            state,
        } => s.voice_update(guild_id, user_id, state),
        Event::VoiceSnapshot { guild_ids, states } => s.voice_snapshot(&guild_ids, states),
        Event::VoiceMoved { channel_id, .. } => s.voice_moved(channel_id),
        Event::ChannelCreated(c) => add_channel(s, c),
        Event::DmOpened(dm) => add_dm(s, dm),
        Event::GuildMemberJoined { guild_id, user } => {
            // Before the list is loaded the join is part of it; a list of one would pass for it.
            if let Some(list) = s.data.members.get_mut(&guild_id)
                && !list.contains(&user.id)
            {
                list.push(user.id);
            }
            s.data.users.insert(user.id, user);
        }
        Event::MessageCreated(m) => {
            if message_created(s, m) {
                // A live arrival in the open channel enters like a sent one, and a new mention
                // badge pops.
                tx.animated();
            }
        }
    }
}

/// A message changed on the server (its embeds were resolved, an admin removed a file). Takes the
/// new content, attachments and embeds; your reactions stay as they are (events keep those
/// current). Returns whether the open channel shows it.
pub fn message_updated(s: &mut AppState, m: Message) -> bool {
    let channel = m.channel_id;
    let visible = s.screen == Screen::Main && s.current_channel() == Some(channel);
    let Some(slot) = s
        .data
        .logs
        .get_mut(&channel)
        .and_then(|l| l.messages.iter_mut().find(|x| x.id == m.id))
    else {
        return false;
    };
    slot.content = m.content;
    slot.edited_at = m.edited_at;
    slot.attachments = m.attachments;
    slot.embeds = m.embeds;
    visible
}

/// Files an incoming message. Returns whether its commit should animate: it landed in the
/// channel being read, or it raised a mention badge.
pub fn message_created(s: &mut AppState, m: Message) -> bool {
    let channel = m.channel_id;
    if !m.nonce.is_empty() {
        s.data.outgoing.retain(|o| o.nonce != m.nonce);
    }
    s.want_author(&m);
    let visible = s.screen == Screen::Main && s.current_channel() == Some(channel);
    let mine = m.author_id == s.my_id();
    let mention = !mine && (s.dm(channel).is_some() || s.mentions_me(&m.content));
    if let Some(log) = s.data.logs.get_mut(&channel).filter(|l| l.loaded) {
        log.insert(m);
    }
    let mut animate = visible;
    if visible {
        if !s.ui.at_bottom && !mine {
            s.ui.unseen_below += 1;
        }
    } else if !mine {
        let u = s.ui.unread.entry(channel).or_default();
        u.count += 1;
        u.mentions += u32::from(mention);
        animate = mention;
    }
    animate
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::format::Tz;
    use crate::state::Unread;

    fn user(id: u64, name: &str) -> User {
        User {
            id,
            username: name.into(),
            created_at: UNIX_EPOCH,
            avatar: None,
            is_admin: false,
            email: None,
        }
    }

    fn msg(id: u64, channel: u64, author: u64, content: &str, nonce: &str) -> Message {
        Message {
            id,
            channel_id: channel,
            guild_id: Some(1),
            author_id: author,
            content: content.into(),
            created_at: UNIX_EPOCH + Duration::from_secs(id),
            edited_at: None,
            nonce: nonce.into(),
            reactions: vec![],
            attachments: vec![],
            embeds: vec![],
        }
    }

    fn state() -> AppState {
        let mut s = AppState::new(|| UNIX_EPOCH, Tz::Fixed(0));
        let me = user(1, "tim");
        s.session = Some(Session {
            token: "t".into(),
            me: me.clone(),
        });
        s.data.users.insert(1, me);
        s.data.users.insert(2, user(2, "ana"));
        s.data.guilds.push(Guild {
            id: 1,
            name: "G".into(),
            owner_id: 1,
            invite_code: "abc".into(),
            created_at: UNIX_EPOCH,
            icon: None,
        });
        let channel = |id: u64, name: &str, position: i32| Channel {
            id,
            guild_id: 1,
            name: name.into(),
            created_at: UNIX_EPOCH,
            section_id: None,
            position,
            icon: Default::default(),
            icon_ref: Default::default(),
            color: Default::default(),
            topic: String::new(),
            kind: ChannelKind::Text,
        };
        s.data
            .channels
            .insert(1, vec![channel(10, "general", 0), channel(11, "dev", 1)]);
        s.screen = Screen::Main;
        s.ui.place = Place::Guild(1);
        s.ui.guild_channel.insert(1, 10);
        s
    }

    fn loaded(s: &mut AppState, channel: u64, ids: &[u64]) {
        let log = s.data.logs.entry(channel).or_default();
        log.insert_page(ids.iter().map(|&i| msg(i, channel, 2, "x", "")).collect());
        log.loaded = true;
    }

    #[test]
    fn the_reply_and_the_gateway_echo_make_one_message() {
        let mut s = state();
        loaded(&mut s, 10, &[1, 2]);
        assert!(s.send("hello"));
        let nonce = s.data.outgoing[0].nonce.clone();
        // Echo first, then the RPC reply.
        message_created(&mut s, msg(3, 10, 1, "hello", &nonce));
        assert!(
            s.data.outgoing.is_empty(),
            "the echo settles the pending message"
        );
        sent(&mut s, 10, &nonce, Ok(msg(3, 10, 1, "hello", &nonce)));
        assert_eq!(
            s.log(10)
                .unwrap()
                .messages
                .iter()
                .filter(|m| m.id == 3)
                .count(),
            1
        );
        // Reply first, then the echo.
        assert!(s.send("again"));
        let nonce = s.data.outgoing[0].nonce.clone();
        sent(&mut s, 10, &nonce, Ok(msg(4, 10, 1, "again", &nonce)));
        message_created(&mut s, msg(4, 10, 1, "again", &nonce));
        assert_eq!(s.log(10).unwrap().messages.len(), 4);
        assert!(s.data.outgoing.is_empty());
    }

    #[test]
    fn a_failed_send_waits_for_retry() {
        let mut s = state();
        loaded(&mut s, 10, &[1]);
        s.send("hi");
        let nonce = s.data.outgoing[0].nonce.clone();
        s.outbox.clear();
        sent(&mut s, 10, &nonce, Err(ApiError::Network("down".into())));
        assert_eq!(s.data.outgoing[0].state, Delivery::Failed);
        s.retry(&nonce);
        assert_eq!(s.data.outgoing[0].state, Delivery::Pending);
        assert_eq!(
            s.outbox,
            vec![Command::Send {
                channel: 10,
                content: "hi".into(),
                nonce
            }]
        );
    }

    #[test]
    fn unread_counts_only_hidden_channels_and_visiting_clears_them() {
        let mut s = state();
        loaded(&mut s, 10, &[1]);
        loaded(&mut s, 11, &[1, 2, 3]);
        message_created(&mut s, msg(5, 11, 2, "plain", ""));
        message_created(&mut s, msg(6, 11, 2, "hey @tim", ""));
        message_created(&mut s, msg(7, 10, 2, "visible", ""));
        assert_eq!(
            s.unread_of(11),
            Unread {
                count: 2,
                mentions: 1
            }
        );
        assert_eq!(s.unread_of(10), Unread::default());
        assert_eq!(s.guild_unread(1).mentions, 1);
        s.select_channel(11);
        assert_eq!(s.unread_of(11), Unread::default());
        // The divider sits after the last message that was already read.
        assert_eq!(
            s.ui.marker,
            Marker::After {
                channel: 11,
                id: Some(3)
            }
        );
        s.select_channel(10);
        assert_eq!(s.ui.marker, Marker::None);
    }

    #[test]
    fn older_pages_prepend_and_stop_at_the_end() {
        let mut s = state();
        loaded(&mut s, 10, &[10, 11, 12]);
        {
            let log = s.data.logs.get_mut(&10).unwrap();
            log.has_more = true;
        }
        s.outbox.clear();
        s.load_older(10);
        s.load_older(10);
        assert_eq!(
            s.outbox,
            vec![Command::LoadMessages {
                channel: 10,
                before: Some(10)
            }],
            "asked once while one is in flight"
        );
        let log = s.data.logs.get_mut(&10).unwrap();
        log.insert_page(vec![msg(9, 10, 2, "a", ""), msg(8, 10, 2, "b", "")]);
        log.fetch = Fetch::Idle;
        log.has_more = false;
        assert_eq!(log.oldest_id, Some(8));
        assert_eq!(
            log.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![8, 9, 10, 11, 12]
        );
        s.outbox.clear();
        s.load_older(10);
        assert!(s.outbox.is_empty(), "no more pages");
    }

    #[test]
    fn entering_an_unloaded_channel_fetches_it_once() {
        let mut s = state();
        s.select_channel(11);
        s.select_channel(10);
        s.select_channel(11);
        let loads = s
            .outbox
            .iter()
            .filter(|c| matches!(c, Command::LoadMessages { .. }))
            .count();
        assert_eq!(loads, 2, "one per channel, not one per visit");
    }

    #[test]
    fn drafts_belong_to_their_channel() {
        let mut s = state();
        s.composer_mut().replace_text("draft in general");
        s.select_channel(11);
        assert_eq!(s.composer().text, "");
        s.select_channel(10);
        assert_eq!(s.composer().text, "draft in general");
    }

    #[test]
    fn over_long_and_blank_messages_are_refused() {
        let mut s = state();
        assert!(!s.send("   "));
        assert!(!s.send(&"x".repeat(thehideout_api::MAX_MESSAGE_CHARS + 1)));
        assert!(s.send(&"x".repeat(thehideout_api::MAX_MESSAGE_CHARS)));
    }

    #[test]
    fn a_stranger_asks_for_the_guild_members_once() {
        let mut s = state();
        s.outbox.clear();
        message_created(&mut s, msg(5, 10, 9, "hi", ""));
        message_created(&mut s, msg(6, 10, 9, "again", ""));
        assert_eq!(s.outbox, vec![Command::LoadMembers { guild: 1 }]);
        // A refused answer allows another try; a known author never asks.
        apply(
            &mut s,
            Reply::Members {
                guild: 1,
                result: Err(ApiError::Network("x".into())),
            },
            &mut Tx::default(),
        );
        message_created(&mut s, msg(7, 10, 9, "hm", ""));
        assert_eq!(s.outbox.len(), 2);
        apply(
            &mut s,
            Reply::Members {
                guild: 1,
                result: Ok(vec![user(9, "zed")]),
            },
            &mut Tx::default(),
        );
        message_created(&mut s, msg(8, 10, 9, "ok", ""));
        assert_eq!(s.outbox.len(), 2);
        assert_eq!(s.user_name(9), "zed");
    }

    #[test]
    fn resuming_signs_in_or_shows_the_form() {
        let mut tx = Tx::default();
        let mut s = AppState::new(|| UNIX_EPOCH, Tz::Fixed(0));
        s.screen = Screen::Loading;
        let session = Session {
            token: "t".into(),
            me: user(1, "tim"),
        };
        apply(
            &mut s,
            Reply::Resume(Ok(Some(("http://example:1".into(), session)))),
            &mut tx,
        );
        assert_eq!(
            (s.screen, s.server_url().as_str()),
            (Screen::Loading, "http://example:1")
        );
        assert_eq!(s.outbox, vec![Command::LoadInitial]);
        // The token is refused when the data is fetched: back to the form, no complaint.
        apply(
            &mut s,
            Reply::Initial(Err(ApiError::Unauthenticated)),
            &mut tx,
        );
        assert_eq!((s.screen, s.auth.error.clone()), (Screen::Auth, None));
        s.screen = Screen::Loading;
        apply(&mut s, Reply::Resume(Ok(None)), &mut tx);
        assert_eq!(s.screen, Screen::Auth);
        s.screen = Screen::Loading;
        apply(
            &mut s,
            Reply::Resume(Err(ApiError::Network("down".into()))),
            &mut tx,
        );
        assert_eq!((s.screen, s.auth.error.is_some()), (Screen::Auth, true));
    }

    #[test]
    fn signing_out_still_tells_the_backend() {
        let mut s = state();
        s.outbox.clear();
        s.sign_out();
        assert_eq!(
            (s.screen, s.outbox.clone()),
            (Screen::Auth, vec![Command::SignOut])
        );
    }

    #[test]
    fn dialog_validation_sends_nothing() {
        let mut s = state();
        s.open_dialog(DialogKind::CreateGuild);
        s.submit_dialog();
        assert!(s.outbox.is_empty());
        assert!(s.ui.dialog.as_ref().unwrap().error.is_some());
        s.open_dialog(DialogKind::NewDm);
        s.ui.dialog.as_mut().unwrap().input.replace_text("@Ana");
        s.submit_dialog();
        assert_eq!(s.outbox, vec![Command::OpenDm { user_id: 2 }]);
    }
}
