//! The state behind the moderation controls: the message being edited, the confirmation dialogs
//! (delete a message, kick, ban, leave, delete a channel or a server, hand a server over, reset
//! the invite code) and what each sends. The rules are in `moderation.rs`; this decides when a
//! control may skip its question (Shift on your own message's Delete) and what a dialog's
//! button does once its fields are right.

use rune::TextEdit;
use thehideout_api::{MAX_MESSAGE_CHARS, Permissions};
use thehideout_design::components::ToastKind;

use crate::forms::{DialogKind, Phase};
use crate::format::{clock_label, day_label, unix};
use crate::state::{AppState, Menu};

/// The message being edited in place of its text.
pub struct Editing {
    pub channel: u64,
    pub message: u64,
    pub input: TextEdit,
}

/// The longest ban reason the server takes.
const MAX_REASON_CHARS: usize = 512;

/// "Delete their recent messages" on the Ban dialog: the label and the seconds it asks for (the
/// server accepts up to 7 days).
pub const CLEANUP: [(&str, u32); 4] = [
    ("Don't delete", 0),
    ("Last hour", 3_600),
    ("Last 24 hours", 86_400),
    ("Last 7 days", 604_800),
];

/// The fields the moderation dialogs add to a `DialogState`.
#[derive(Default)]
pub struct ModForm {
    /// The Ban dialog's choice in `CLEANUP`, and whether the select is open.
    pub cleanup: usize,
    pub cleanup_open: bool,
    /// The member a hand-over goes to, and whether the select is open.
    pub target: Option<u64>,
    pub target_open: bool,
    /// The Invite dialog is asking whether to reset the code.
    pub resetting: bool,
}

impl AppState {
    // ---- editing a message -------------------------------------------------------------------

    /// The message being edited, when it is in the open channel.
    pub fn editing_here(&self) -> Option<&Editing> {
        let e = self.ui.editing.as_ref()?;
        (self.current_channel() == Some(e.channel)).then_some(e)
    }

    /// Starts editing our message `message` of `channel`.
    pub fn begin_edit(&mut self, channel: u64, message: u64) -> bool {
        let Some(m) = self.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)) else {
            return false;
        };
        if !self.can_edit_message(m) {
            return false;
        }
        let mut input = TextEdit::new("");
        input.replace_text(m.content.clone());
        input.focused = true;
        self.ui.menu = Menu::None;
        self.ui.editing = Some(Editing { channel, message, input });
        true
    }

    /// The Edit button or menu item of the message with this transcript id.
    pub fn begin_edit_chat(&mut self, chat_id: &str) -> bool {
        match self.message_ref(chat_id) {
            Some((channel, message)) => self.begin_edit(channel, message),
            None => false,
        }
    }

    /// Up in an empty composer: edits your latest message of the open channel.
    pub fn edit_last(&mut self) -> bool {
        let Some(channel) = self.current_channel() else { return false };
        if !self.composer().text.is_empty() || self.ui.editing.is_some() {
            return false;
        }
        let me = self.my_id();
        let last = self.log(channel).and_then(|l| l.messages.iter().rev().find(|m| m.author_id == me)).map(|m| m.id);
        match last {
            Some(id) => self.begin_edit(channel, id),
            None => false,
        }
    }

    pub fn cancel_edit(&mut self) {
        self.ui.editing = None;
    }

    /// Enter in the editor: sends the new text. Unchanged text just closes the editor; empty text
    /// or too much stays open.
    pub fn save_edit(&mut self) {
        let Some(e) = &self.ui.editing else { return };
        let (channel, message) = (e.channel, e.message);
        let text = e.input.text.trim().to_owned();
        let same = self.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)).is_none_or(|m| m.content == text);
        if same {
            self.ui.editing = None;
            return;
        }
        if text.chars().count() > MAX_MESSAGE_CHARS {
            self.toast(ToastKind::Danger, format!("Messages can be up to {MAX_MESSAGE_CHARS} characters."));
            return;
        }
        if self.edit_message(channel, message, &text) {
            self.ui.editing = None;
        }
    }

    /// The hover text of an "(edited)" mark: "Edited today at 14:05".
    pub fn edited_tip(&self, at: std::time::SystemTime) -> String {
        let t = unix(at);
        format!("Edited {} at {}", day_label(self.day_of(t), self.today).to_lowercase(), clock_label(self.tz, t))
    }

    // ---- deleting a message ------------------------------------------------------------------

    /// The Delete button, menu item or key of a message. Asks first, except on your own message
    /// with Shift held.
    pub fn delete_pressed(&mut self, channel: u64, message: u64, shift: bool) {
        let Some(m) = self.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)) else { return };
        if !self.can_delete_message(m) {
            return;
        }
        if shift && m.author_id == self.my_id() {
            self.ui.menu = Menu::None;
            self.delete_message(channel, message);
            return;
        }
        self.open_dialog(DialogKind::DeleteMessage { channel, message });
    }

    /// [`delete_pressed`](Self::delete_pressed) by transcript id.
    pub fn delete_pressed_chat(&mut self, chat_id: &str, shift: bool) {
        if let Some((channel, message)) = self.message_ref(chat_id) {
            self.delete_pressed(channel, message, shift);
        }
    }

    // ---- members -----------------------------------------------------------------------------

    /// Whether the user may kick or ban at all (so the people page is theirs to open).
    pub fn can_remove_people(&self, guild: u64) -> bool {
        let p = self.perms(guild);
        p.contains(Permissions::KICK_MEMBERS) || p.contains(Permissions::BAN_MEMBERS)
    }

    /// Whether any moderation entry is on offer for `user` in `guild`.
    pub fn has_member_menu(&self, guild: u64, user: u64) -> bool {
        self.can_kick(guild, user) || self.can_ban(guild, user)
    }

    /// Opens the Ban dialog / the Kick dialog for `user`.
    pub fn ask_kick(&mut self, guild: u64, user: u64) {
        if self.can_kick(guild, user) {
            self.open_dialog(DialogKind::KickMember { guild, user });
        }
    }

    pub fn ask_ban(&mut self, guild: u64, user: u64) {
        if self.can_ban(guild, user) {
            self.open_dialog(DialogKind::BanMember { guild, user });
        }
    }

    // ---- the dialogs' buttons ----------------------------------------------------------------

    /// The members a server can be handed to (everyone but us), by name.
    pub fn transfer_candidates(&self, guild: u64) -> Vec<(u64, String)> {
        let me = self.my_id();
        let mut v: Vec<(u64, String)> = self
            .data
            .members
            .get(&guild)
            .into_iter()
            .flatten()
            .filter(|u| **u != me)
            .map(|u| (*u, self.user_name(*u)))
            .collect();
        v.sort_by_key(|(_, n)| n.to_lowercase());
        v
    }

    /// Whether the name typed in the open dialog is the server's.
    pub fn typed_name_matches(&self, guild: u64) -> bool {
        let typed = self.ui.dialog.as_ref().map(|d| d.input.text.trim().to_owned());
        typed.is_some_and(|t| self.guild(guild).is_some_and(|g| g.name == t))
    }

    /// Enter or the primary button of a moderation dialog. Returns whether it was one.
    pub(crate) fn submit_moderation_dialog(&mut self) -> bool {
        let Some(d) = self.ui.dialog.as_ref() else { return false };
        let kind = d.kind;
        let (cleanup, target, resetting) = (d.form.cleanup, d.form.target, d.form.resetting);
        let reason: String = d.input.text.chars().take(MAX_REASON_CHARS).collect();
        if d.phase == Phase::Busy {
            return matches!(
                kind,
                DialogKind::DeleteMessage { .. }
                    | DialogKind::KickMember { .. }
                    | DialogKind::BanMember { .. }
                    | DialogKind::LeaveGuild { .. }
                    | DialogKind::DeleteChannel { .. }
                    | DialogKind::DeleteGuild { .. }
                    | DialogKind::TransferOwnership { .. }
            );
        }
        let wrong_name = |s: &mut AppState, guild: u64| {
            if !s.typed_name_matches(guild) {
                if let Some(d) = s.ui.dialog.as_mut() {
                    d.error = Some("Type the server's name exactly to confirm.".to_owned());
                }
                return true;
            }
            false
        };
        match kind {
            DialogKind::DeleteMessage { channel, message } => {
                self.ui.dialog = None;
                self.delete_message(channel, message);
            }
            DialogKind::KickMember { guild, user } => {
                self.ui.dialog = None;
                self.kick(guild, user);
            }
            DialogKind::BanMember { guild, user } => {
                self.ui.dialog = None;
                let seconds = CLEANUP.get(cleanup).map_or(0, |c| c.1);
                self.ban(guild, user, &reason, (seconds > 0).then_some(seconds));
            }
            DialogKind::LeaveGuild { guild } => {
                self.ui.dialog = None;
                self.leave_guild(guild);
            }
            DialogKind::DeleteChannel { guild, channel } => {
                self.ui.dialog = None;
                self.delete_channel(guild, channel);
            }
            DialogKind::DeleteGuild { guild } => {
                if wrong_name(self, guild) {
                    return true;
                }
                self.ui.dialog = None;
                self.delete_guild(guild, &reason);
            }
            DialogKind::TransferOwnership { guild } => {
                let Some(user) = target else {
                    if let Some(d) = self.ui.dialog.as_mut() {
                        d.error = Some("Pick who should own the server.".to_owned());
                    }
                    return true;
                };
                if wrong_name(self, guild) {
                    return true;
                }
                self.ui.dialog = None;
                self.transfer_ownership(guild, user, &reason);
            }
            DialogKind::Invite { guild } if resetting => {
                if let Some(d) = self.ui.dialog.as_mut() {
                    d.form.resetting = false;
                }
                self.rotate_invite(guild);
            }
            _ => return false,
        }
        true
    }

    /// The Invite dialog's "Reset code" and its "Back".
    pub fn ask_reset_invite(&mut self, on: bool) {
        if let Some(d) = self.ui.dialog.as_mut()
            && matches!(d.kind, DialogKind::Invite { .. })
        {
            d.form.resetting = on;
        }
    }

    // ---- what the server does to us while we look --------------------------------------------

    /// A message of the transcript is gone: an edit of it ends (and says why).
    pub(crate) fn editing_message_gone(&mut self, message: u64) {
        if self.ui.editing.as_ref().is_some_and(|e| e.message == message) {
            self.ui.editing = None;
            self.toast(ToastKind::Info, "The message you were editing was deleted.");
        }
    }

    /// A channel is gone: so is an edit in it.
    pub(crate) fn editing_channel_gone(&mut self, channel: u64) {
        if self.ui.editing.as_ref().is_some_and(|e| e.channel == channel) {
            self.ui.editing = None;
        }
    }
}
