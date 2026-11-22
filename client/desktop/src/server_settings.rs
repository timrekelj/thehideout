//! Server settings and profile pictures on `AppState`: who may do what to a guild's roles (the
//! server's rules, mirrored so the controls can be greyed out), the page state of the settings
//! screen (overview, roles, members), the optimistic role reorder with its undo, and the picked
//! pictures for an avatar or a server icon. Pure state and commands, like the rest of the model.

use std::path::Path;

use rune::TextEdit;
use thehideout_api::{ApiError, Guild, PaletteColor, Permissions, Role};

use crate::backend::{Command, ImagePurpose};
use crate::emoji::EmojiPage;
use crate::forms::{DialogKind, Phase, check_name, sentence};
use crate::media;
use crate::state::AppState;

pub const ROLES_CONFLICT_TOAST: &str = "Roles changed elsewhere \u{2014} try again";
/// Points between role rows in the list (a row and its gap): what a drag of `dy` steps over.
pub const ROLE_PITCH: f32 = 42.0;
pub const MAX_ROLE_NAME: usize = 32;

/// One switch of the role editor.
pub struct PermRow {
    pub bit: Permissions,
    pub name: &'static str,
    pub help: &'static str,
}

pub const PERMISSION_ROWS: [PermRow; 6] = [
    PermRow { bit: Permissions::ADMINISTRATOR, name: "Administrator", help: "Grants every permission below. Give it only to people you trust completely." },
    PermRow { bit: Permissions::MANAGE_SERVER, name: "Manage server", help: "Rename the server and change its icon." },
    PermRow { bit: Permissions::MANAGE_CHANNELS, name: "Manage channels", help: "Create and edit channels and sections, and reorder the sidebar." },
    PermRow { bit: Permissions::MANAGE_ROLES, name: "Manage roles", help: "Create, edit, reorder and hand out roles below their own highest role." },
    PermRow { bit: Permissions::MANAGE_EMOJIS, name: "Manage emojis", help: "Add, rename and remove custom emojis." },
    PermRow { bit: Permissions::INVITE_PEOPLE, name: "Invite people", help: "Invite people by email to make an account and join this server, and withdraw or resend this server's invites." },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerPage {
    Overview,
    Roles,
    Members,
    /// The ban list (BAN_MEMBERS).
    Bans,
    Emojis,
}

/// Picked bytes waiting to be saved, with a decoded preview.
pub struct ImageDraft {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub preview: rune::Image,
}

/// A picture being chosen for an avatar or a server icon: the file is read off-thread (`reading`),
/// then shown (`draft`) until saved or dropped.
#[derive(Default)]
pub struct ImagePick {
    pub draft: Option<ImageDraft>,
    pub reading: bool,
    /// A file is being dragged over the drop zone.
    pub drop_over: bool,
}

/// The role editor's draft: what the switches and fields show before "Save".
pub struct RoleForm {
    /// `None` while creating.
    pub role: Option<u64>,
    pub name: TextEdit,
    pub color: PaletteColor,
    pub perms: Permissions,
    pub original: Option<Role>,
    pub error: Option<String>,
    pub busy: bool,
}

impl RoleForm {
    pub fn blank() -> RoleForm {
        let mut name = TextEdit::new("New role");
        name.focused = true;
        RoleForm { role: None, name, color: PaletteColor::None, perms: Permissions::NONE, original: None, error: None, busy: false }
    }

    pub fn of(role: &Role) -> RoleForm {
        RoleForm { role: Some(role.id), name: TextEdit::new(&role.name), color: role.color, perms: role.permissions, original: Some(role.clone()), error: None, busy: false }
    }

    /// Whether anything differs from what was loaded (a new role always does).
    pub fn dirty(&self) -> bool {
        match &self.original {
            None => true,
            Some(o) => self.name.text.trim() != o.name || self.color != o.color || self.perms != o.permissions,
        }
    }
}

/// A role row held by the pointer, `dy` points from its place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoleDrag {
    pub guild: u64,
    pub role: u64,
    pub dy: f32,
}

pub struct ServerSettings {
    pub guild: u64,
    pub page: ServerPage,
    pub name: TextEdit,
    pub name_error: Option<String>,
    pub name_busy: bool,
    pub icon: ImagePick,
    pub icon_error: Option<String>,
    pub icon_busy: bool,
    pub form: Option<RoleForm>,
    /// A refusal of a reorder, shown over the list.
    pub role_error: Option<String>,
    pub drag: Option<RoleDrag>,
    pub search: TextEdit,
    /// The member whose "+ role" menu is open.
    pub member_menu: Option<u64>,
    pub member_error: Option<String>,
    pub member_busy: Option<u64>,
    /// The Emojis page: the draft being named, a rename in progress, errors.
    pub emojis: EmojiPage,
}

/// A header of the member panel and the members under it.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberGroup {
    /// `None`: the members without a role.
    pub role: Option<Role>,
    pub users: Vec<u64>,
}

/// What an API error says in a form or a banner.
pub fn error_text(e: &ApiError) -> String {
    match e {
        ApiError::PermissionDenied => "You don't have permission to do that.".to_owned(),
        ApiError::NotFound => "That no longer exists.".to_owned(),
        ApiError::AlreadyExists => "That name is already taken here.".to_owned(),
        ApiError::Conflict => "That changed while you were editing. Try again.".to_owned(),
        ApiError::InvalidArgument(m) => sentence(m),
        ApiError::TooLarge => "That image is over 8 MiB.".to_owned(),
        ApiError::Network(_) => "Can't reach the server.".to_owned(),
        other => sentence(&other.to_string()),
    }
}

/// Roles most senior first (the order the server lists them in).
pub fn sorted(mut roles: Vec<Role>) -> Vec<Role> {
    roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    roles
}

impl AppState {
    // ---- who may do what ---------------------------------------------------------------------

    pub fn is_owner(&self, guild: u64) -> bool {
        self.guild(guild).is_some_and(|g| g.owner_id == self.my_id())
    }

    pub fn roles_of(&self, guild: u64) -> &[Role] {
        self.data.roles.get(&guild).map_or(&[], Vec::as_slice)
    }

    pub fn role(&self, guild: u64, id: u64) -> Option<&Role> {
        self.roles_of(guild).iter().find(|r| r.id == id)
    }

    pub fn role_version(&self, guild: u64) -> u64 {
        self.data.role_versions.get(&guild).copied().unwrap_or(0)
    }

    /// The role ids `user` holds in `guild`.
    pub fn member_role_ids(&self, guild: u64, user: u64) -> &[u64] {
        self.data.member_roles.get(&guild).and_then(|m| m.get(&user)).map_or(&[], Vec::as_slice)
    }

    /// The position of the signed-in user's highest role (`None` without roles).
    pub fn top_position(&self, guild: u64) -> Option<i32> {
        let mine = self.member_role_ids(guild, self.my_id());
        self.roles_of(guild).iter().filter(|r| mine.contains(&r.id)).map(|r| r.position).max()
    }

    /// Whether the user may manage a role at `position`: the owner always, anyone else only
    /// strictly below their highest role (`proto/README.md`).
    pub fn outranks(&self, guild: u64, position: i32) -> bool {
        self.is_owner(guild) || self.top_position(guild).is_some_and(|top| position < top)
    }

    pub fn can_manage_roles(&self, guild: u64) -> bool {
        self.perms(guild).contains(Permissions::MANAGE_ROLES)
    }

    pub fn can_manage_server(&self, guild: u64) -> bool {
        self.perms(guild).contains(Permissions::MANAGE_SERVER)
    }

    /// "Server settings…" is offered to the owner and to anyone who may manage the server or its roles.
    pub fn can_open_server_settings(&self, guild: u64) -> bool {
        self.can_manage_server(guild)
            || self.can_manage_roles(guild)
            || self.can_manage_emojis(guild)
            || self.can_remove_people(guild)
    }

    /// Whether the user may edit, delete or hand out `role`.
    pub fn can_edit_role(&self, guild: u64, role: &Role) -> bool {
        self.can_manage_roles(guild) && self.outranks(guild, role.position)
    }

    /// Whether the user may grant or revoke every bit of `bits` (they must hold them; the owner holds all).
    pub fn can_grant(&self, guild: u64, bits: Permissions) -> bool {
        self.is_owner(guild) || self.perms(guild).contains(bits)
    }

    /// The colour of a member's name: that of their highest-positioned role that has one.
    pub fn name_color(&self, guild: u64, user: u64) -> PaletteColor {
        // A deleted account has no role colour to show: its name is the plain placeholder.
        if self.data.users.get(&user).is_some_and(|u| u.deleted) {
            return PaletteColor::None;
        }
        let mine = self.member_role_ids(guild, user);
        self.roles_of(guild).iter().filter(|r| mine.contains(&r.id) && r.color != PaletteColor::None).max_by_key(|r| r.position).map_or(PaletteColor::None, |r| r.color)
    }

    /// How many of the guild's loaded members hold `role`.
    pub fn role_members(&self, guild: u64, role: u64) -> usize {
        self.data.member_roles.get(&guild).map_or(0, |m| m.values().filter(|ids| ids.contains(&role)).count())
    }

    /// The member panel's groups: one per role that is somebody's highest (by position), then
    /// the members with no role. Names sort case-insensitively inside a group.
    pub fn member_groups(&self, guild: u64) -> Vec<MemberGroup> {
        let ids = self.data.members.get(&guild).cloned().unwrap_or_default();
        let roles = self.roles_of(guild);
        let mut by_role: Vec<(Role, Vec<u64>)> = Vec::new();
        let mut rest = Vec::new();
        for id in ids {
            let held = self.member_role_ids(guild, id);
            match roles.iter().filter(|r| held.contains(&r.id)).max_by_key(|r| r.position) {
                Some(top) => match by_role.iter_mut().find(|(r, _)| r.id == top.id) {
                    Some((_, users)) => users.push(id),
                    None => by_role.push((top.clone(), vec![id])),
                },
                None => rest.push(id),
            }
        }
        by_role.sort_by_key(|(r, _)| std::cmp::Reverse(r.position));
        let name = |u: &u64| self.user_name(*u).to_lowercase();
        let mut out: Vec<MemberGroup> = by_role
            .into_iter()
            .map(|(role, mut users)| {
                users.sort_by_key(name);
                MemberGroup { role: Some(role), users }
            })
            .collect();
        rest.sort_by_key(name);
        if !rest.is_empty() {
            out.push(MemberGroup { role: None, users: rest });
        }
        out
    }

    /// The roles `user` doesn't hold that the signed-in user may hand out.
    pub fn assignable_roles(&self, guild: u64, user: u64) -> Vec<Role> {
        let held = self.member_role_ids(guild, user);
        self.roles_of(guild).iter().filter(|r| !held.contains(&r.id) && self.can_edit_role(guild, r)).cloned().collect()
    }

    // ---- the page ----------------------------------------------------------------------------

    pub fn settings_page_allowed(&self, guild: u64, page: ServerPage) -> bool {
        match page {
            ServerPage::Overview => self.can_manage_server(guild),
            ServerPage::Roles => self.can_manage_roles(guild),
            // The people page is for those who hand out roles and those who may remove people.
            ServerPage::Members => {
                self.can_manage_roles(guild) || self.can_remove_people(guild)
            }
            ServerPage::Bans => self.can_view_bans(guild),
            ServerPage::Emojis => self.can_manage_emojis(guild),
        }
    }

    pub fn open_server_settings(&mut self, guild: u64) {
        self.ui.menu = crate::state::Menu::None;
        if !self.can_open_server_settings(guild) {
            return;
        }
        let page = [ServerPage::Overview, ServerPage::Roles, ServerPage::Members, ServerPage::Bans, ServerPage::Emojis].into_iter().find(|p| self.settings_page_allowed(guild, *p)).unwrap_or(ServerPage::Roles);
        let name = self.guild(guild).map_or_else(String::new, |g| g.name.clone());
        self.want_members(guild);
        self.want_access(guild);
        self.want_emojis(guild);
        self.ui.settings = None;
        self.ui.server_settings = Some(ServerSettings {
            guild,
            page,
            name: TextEdit::new(&name),
            name_error: None,
            name_busy: false,
            icon: ImagePick::default(),
            icon_error: None,
            icon_busy: false,
            form: None,
            role_error: None,
            drag: None,
            search: TextEdit::new(""),
            member_menu: None,
            member_error: None,
            member_busy: None,
            emojis: EmojiPage::default(),
        });
    }

    pub fn close_server_settings(&mut self) {
        self.ui.server_settings = None;
    }

    pub fn server_page(&mut self, page: ServerPage) {
        let Some(ss) = self.ui.server_settings.as_ref() else { return };
        if self.settings_page_allowed(ss.guild, page)
            && let Some(ss) = self.ui.server_settings.as_mut()
        {
            ss.page = page;
            ss.member_menu = None;
            ss.drag = None;
            if page == ServerPage::Bans {
                let guild = ss.guild;
                self.load_bans(guild);
            }
        }
    }

    // ---- overview: name and icon -------------------------------------------------------------

    /// "Save" under the name field: validates, then renames (nothing sent when unchanged).
    pub fn rename_guild(&mut self) {
        let Some(ss) = self.ui.server_settings.as_ref() else { return };
        let (guild, name) = (ss.guild, ss.name.text.trim().to_owned());
        if ss.name_busy || !self.can_manage_server(guild) {
            return;
        }
        let unchanged = self.guild(guild).is_some_and(|g| g.name == name);
        let Some(ss) = self.ui.server_settings.as_mut() else { return };
        if unchanged {
            ss.name_error = None;
            return;
        }
        if let Err(e) = check_name(&name, 2, 32, "Give the server a name") {
            ss.name_error = Some(e);
            return;
        }
        ss.name_error = None;
        ss.name_busy = true;
        self.outbox.push(Command::RenameGuild { guild, name });
    }

    /// Opens the file chooser's answer for `purpose`: checks the type and has the backend read it.
    pub fn pick_image(&mut self, purpose: ImagePurpose, path: &Path) {
        if matches!(purpose, ImagePurpose::Emoji { .. }) {
            return self.pick_emoji_file(path);
        }
        if media::image_type(path).is_none() {
            self.image_error(purpose, "Choose a PNG, JPEG, WebP or GIF image.".into());
            return;
        }
        let pick = match purpose {
            ImagePurpose::Avatar => self.ui.dialog.as_mut().map(|d| &mut d.pick),
            ImagePurpose::GuildIcon { guild } => self.ui.server_settings.as_mut().filter(|s| s.guild == guild).map(|s| &mut s.icon),
            ImagePurpose::Emoji { .. } => None,
        };
        let Some(pick) = pick else { return };
        if pick.reading {
            return;
        }
        pick.reading = true;
        self.clear_image_error(purpose);
        self.outbox.push(Command::ReadImage { path: path.to_path_buf(), purpose });
    }

    fn image_error(&mut self, purpose: ImagePurpose, message: String) {
        match purpose {
            ImagePurpose::Avatar => {
                if let Some(d) = self.ui.dialog.as_mut() {
                    d.error = Some(message);
                }
            }
            ImagePurpose::GuildIcon { guild } => {
                if let Some(ss) = self.ui.server_settings.as_mut().filter(|s| s.guild == guild) {
                    ss.icon_error = Some(message);
                }
            }
            ImagePurpose::Emoji { .. } => {}
        }
    }

    fn clear_image_error(&mut self, purpose: ImagePurpose) {
        match purpose {
            ImagePurpose::Avatar => {
                if let Some(d) = self.ui.dialog.as_mut() {
                    d.error = None;
                }
            }
            ImagePurpose::GuildIcon { .. } => {
                if let Some(ss) = self.ui.server_settings.as_mut() {
                    ss.icon_error = None;
                }
            }
            ImagePurpose::Emoji { .. } => {}
        }
    }

    /// The backend's answer to `ReadImage`.
    pub fn image_read(&mut self, purpose: ImagePurpose, result: Result<(Vec<u8>, String), ApiError>) {
        if let ImagePurpose::Emoji { guild } = purpose {
            return self.emoji_file_read(guild, result);
        }
        let pick = match purpose {
            ImagePurpose::Avatar => self.ui.dialog.as_mut().filter(|d| d.kind == DialogKind::Profile).map(|d| &mut d.pick),
            ImagePurpose::GuildIcon { guild } => self.ui.server_settings.as_mut().filter(|s| s.guild == guild).map(|s| &mut s.icon),
            ImagePurpose::Emoji { .. } => None,
        };
        let Some(pick) = pick else { return };
        pick.reading = false;
        match result {
            Ok((bytes, content_type)) => {
                let preview = self.data.media.preview(bytes.clone());
                let pick = match purpose {
                    ImagePurpose::Avatar => self.ui.dialog.as_mut().map(|d| &mut d.pick),
                    ImagePurpose::GuildIcon { .. } => self.ui.server_settings.as_mut().map(|s| &mut s.icon),
                    ImagePurpose::Emoji { .. } => None,
                };
                if let Some(pick) = pick {
                    pick.draft = Some(ImageDraft { bytes, content_type, preview });
                }
                self.clear_image_error(purpose);
            }
            Err(e) => self.image_error(purpose, error_text(&e)),
        }
    }

    /// "Save icon": uploads the picked picture.
    pub fn save_icon(&mut self) {
        let Some(ss) = self.ui.server_settings.as_mut() else { return };
        let guild = ss.guild;
        if ss.icon_busy {
            return;
        }
        let Some(d) = ss.icon.draft.as_ref() else { return };
        let cmd = Command::UploadGuildIcon { guild, bytes: d.bytes.clone(), content_type: d.content_type.clone() };
        ss.icon_busy = true;
        ss.icon_error = None;
        self.outbox.push(cmd);
    }

    pub fn discard_icon(&mut self) {
        if let Some(ss) = self.ui.server_settings.as_mut()
            && !ss.icon_busy
        {
            ss.icon.draft = None;
            ss.icon_error = None;
        }
    }

    pub fn remove_icon(&mut self) {
        let Some(ss) = self.ui.server_settings.as_mut() else { return };
        if ss.icon_busy {
            return;
        }
        ss.icon_busy = true;
        ss.icon_error = None;
        let guild = ss.guild;
        self.outbox.push(Command::RemoveGuildIcon { guild });
    }

    // ---- profile -----------------------------------------------------------------------------

    pub fn open_profile(&mut self) {
        self.open_dialog(DialogKind::Profile);
    }

    /// The Profile dialog's "Remove" button.
    pub fn remove_avatar(&mut self) {
        if self.me().is_none_or(|u| u.avatar.is_none()) {
            return;
        }
        let Some(d) = self.ui.dialog.as_mut().filter(|d| d.kind == DialogKind::Profile) else { return };
        if d.phase == Phase::Busy {
            return;
        }
        d.phase = Phase::Busy;
        d.error = None;
        self.outbox.push(Command::RemoveAvatar);
    }

    /// Drops a picked picture the dialog was holding.
    pub fn discard_avatar_draft(&mut self) {
        if let Some(d) = self.ui.dialog.as_mut().filter(|d| d.kind == DialogKind::Profile && d.phase == Phase::Idle) {
            d.pick.draft = None;
        }
    }

    // ---- roles -------------------------------------------------------------------------------

    pub fn select_role(&mut self, role: u64) {
        let Some(ss) = self.ui.server_settings.as_ref() else { return };
        let Some(r) = self.role(ss.guild, role).cloned() else { return };
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.form = Some(RoleForm::of(&r));
            ss.role_error = None;
        }
    }

    pub fn new_role(&mut self) {
        let Some(ss) = self.ui.server_settings.as_ref() else { return };
        if !self.can_manage_roles(ss.guild) {
            return;
        }
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.form = Some(RoleForm::blank());
            ss.role_error = None;
        }
    }

    pub fn close_role_form(&mut self) {
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.form = None;
        }
    }

    pub fn role_form(&mut self) -> Option<&mut RoleForm> {
        self.ui.server_settings.as_mut().and_then(|s| s.form.as_mut())
    }

    /// Whether the editor may change the form's role (or create one): not a role at or above
    /// the user's highest.
    pub fn form_editable(&self) -> bool {
        let Some(ss) = self.ui.server_settings.as_ref() else { return false };
        match ss.form.as_ref().and_then(|f| f.role) {
            None => self.can_manage_roles(ss.guild),
            Some(id) => self.role(ss.guild, id).is_some_and(|r| self.can_edit_role(ss.guild, r)),
        }
    }

    pub fn set_role_color(&mut self, key: &str) {
        if !self.form_editable() {
            return;
        }
        if let Some(f) = self.role_form() {
            f.color = PaletteColor::from_key(key);
        }
    }

    /// A switch of the editor: flips `bit` unless the user doesn't hold it (the server refuses a
    /// grant or revoke of a bit you don't have).
    pub fn toggle_permission(&mut self, bit: Permissions) {
        let Some(guild) = self.ui.server_settings.as_ref().map(|s| s.guild) else { return };
        if !self.form_editable() || !self.can_grant(guild, bit) {
            return;
        }
        if let Some(f) = self.role_form() {
            f.perms = Permissions(f.perms.bits() ^ bit.bits());
        }
    }

    /// "Save" / "Create role" in the editor.
    pub fn save_role(&mut self) {
        let Some(ss) = self.ui.server_settings.as_ref() else { return };
        let guild = ss.guild;
        if !self.form_editable() {
            return;
        }
        let Some(f) = ss.form.as_ref() else { return };
        if f.busy {
            return;
        }
        let name = f.name.text.trim().to_owned();
        let cmd = match &f.original {
            None => {
                if let Err(e) = check_name(&name, 1, MAX_ROLE_NAME, "Give the role a name") {
                    return self.role_form_error(e);
                }
                Command::CreateRole { guild, name, color: f.color, permissions: f.perms }
            }
            Some(o) => {
                if let Err(e) = check_name(&name, 1, MAX_ROLE_NAME, "Give the role a name") {
                    return self.role_form_error(e);
                }
                let cmd = Command::UpdateRole {
                    guild,
                    role: o.id,
                    name: (name != o.name).then_some(name),
                    color: (f.color != o.color).then_some(f.color),
                    permissions: (f.perms != o.permissions).then_some(f.perms),
                };
                if matches!(cmd, Command::UpdateRole { name: None, color: None, permissions: None, .. }) {
                    return;
                }
                cmd
            }
        };
        if let Some(f) = self.role_form() {
            f.busy = true;
            f.error = None;
        }
        self.outbox.push(cmd);
    }

    fn role_form_error(&mut self, e: String) {
        if let Some(f) = self.role_form() {
            f.error = Some(e);
        }
    }

    /// The "Delete role" button: asks first.
    pub fn ask_delete_role(&mut self, role: u64) {
        let Some(guild) = self.ui.server_settings.as_ref().map(|s| s.guild) else { return };
        if self.role(guild, role).is_some_and(|r| self.can_edit_role(guild, r)) {
            self.open_dialog(DialogKind::DeleteRole { guild, role });
        }
    }

    // ---- reordering --------------------------------------------------------------------------

    /// The roles as they would be with `role` moved to index `to` (most senior first, positions
    /// renumbered `n-1..0`), or `None` if that is no move or the hierarchy forbids it: a role
    /// that changes position must be below the user's highest both before and after.
    pub fn reordered(&self, guild: u64, role: u64, to: usize) -> Option<Vec<Role>> {
        let roles = self.roles_of(guild);
        let from = roles.iter().position(|r| r.id == role)?;
        if from == to || to >= roles.len() || !self.can_manage_roles(guild) {
            return None;
        }
        let mut order: Vec<Role> = roles.to_vec();
        let moved = order.remove(from);
        order.insert(to, moved);
        let n = order.len() as i32;
        for (i, r) in order.iter_mut().enumerate() {
            let new = n - 1 - i as i32;
            if r.position != new && !(self.outranks(guild, r.position) && self.outranks(guild, new)) {
                return None;
            }
            r.position = new;
        }
        Some(order)
    }

    pub fn can_move_role(&self, guild: u64, role: u64, delta: i32) -> bool {
        let Some(from) = self.roles_of(guild).iter().position(|r| r.id == role) else { return false };
        let to = from as i32 + delta;
        to >= 0 && !self.data.role_undo.contains_key(&guild) && self.reordered(guild, role, to as usize).is_some()
    }

    /// Applies the new order at once and sends it based on the current version. False (nothing
    /// changed) when the move isn't allowed or an earlier one is still waiting for its answer.
    pub fn move_role_to(&mut self, guild: u64, role: u64, to: usize) -> bool {
        if self.data.role_undo.contains_key(&guild) {
            return false;
        }
        let Some(new) = self.reordered(guild, role, to) else { return false };
        let old = self.roles_of(guild).to_vec();
        self.data.role_undo.insert(guild, old);
        let ids = new.iter().map(|r| r.id).collect();
        let expected_version = self.role_version(guild);
        self.data.roles.insert(guild, new);
        self.outbox.push(Command::ReorderRoles { guild, expected_version, role_ids: ids });
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.role_error = None;
        }
        true
    }

    /// The up and down buttons.
    pub fn step_role(&mut self, guild: u64, role: u64, delta: i32) -> bool {
        let Some(from) = self.roles_of(guild).iter().position(|r| r.id == role) else { return false };
        let to = from as i32 + delta;
        to >= 0 && self.move_role_to(guild, role, to as usize)
    }

    /// While the pointer holds a row: it follows by `dy`.
    pub fn drag_role(&mut self, guild: u64, role: u64, dy: f32) {
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.drag = Some(RoleDrag { guild, role, dy });
        }
    }

    /// Where the held row would land, as an index into the list.
    pub fn role_drop_slot(&self) -> Option<usize> {
        let d = self.ui.server_settings.as_ref()?.drag?;
        let roles = self.roles_of(d.guild);
        let from = roles.iter().position(|r| r.id == d.role)?;
        let to = (from as f32 + d.dy / ROLE_PITCH).round().clamp(0.0, roles.len() as f32 - 1.0) as usize;
        Some(to)
    }

    /// The pointer lets go: the row goes where it was dropped, if that's allowed.
    pub fn end_role_drag(&mut self, cancelled: bool) {
        let slot = self.role_drop_slot();
        let Some(d) = self.ui.server_settings.as_mut().and_then(|s| s.drag.take()) else { return };
        let Some(to) = slot.filter(|_| !cancelled) else { return };
        let from = self.roles_of(d.guild).iter().position(|r| r.id == d.role);
        if from == Some(to) || self.data.role_undo.contains_key(&d.guild) {
            return;
        }
        if !self.move_role_to(d.guild, d.role, to)
            && let Some(ss) = self.ui.server_settings.as_mut()
        {
            ss.role_error = Some("You can't move a role to or above your own highest role.".into());
        }
    }

    // ---- members -----------------------------------------------------------------------------

    /// Gives or takes `role` from `user`: sends the member's whole new set. Refused here (nothing
    /// sent) for a role the user may not hand out.
    pub fn set_member_role(&mut self, guild: u64, user: u64, role: u64, on: bool) {
        if self.ui.server_settings.as_ref().is_some_and(|s| s.member_busy.is_some()) {
            return;
        }
        if !self.role(guild, role).is_some_and(|r| self.can_edit_role(guild, r)) {
            return;
        }
        let mut ids = self.member_role_ids(guild, user).to_vec();
        ids.retain(|r| *r != role);
        if on {
            ids.push(role);
        }
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.member_busy = Some(user);
            ss.member_error = None;
            ss.member_menu = None;
        }
        self.outbox.push(Command::SetMemberRoles { guild, user, role_ids: ids });
    }

    pub fn toggle_member_menu(&mut self, user: u64) {
        if let Some(ss) = self.ui.server_settings.as_mut() {
            ss.member_menu = if ss.member_menu == Some(user) { None } else { Some(user) };
        }
    }

    /// Members matching the search box, in the guild's order by name.
    pub fn searched_members(&self, guild: u64) -> Vec<u64> {
        let want = self.ui.server_settings.as_ref().map_or_else(String::new, |s| s.search.text.trim().to_lowercase());
        let mut ids: Vec<u64> = self.data.members.get(&guild).cloned().unwrap_or_default();
        ids.retain(|u| want.is_empty() || self.user_name(*u).to_lowercase().contains(&want));
        ids.sort_by_key(|u| self.user_name(*u).to_lowercase());
        ids
    }

    // ---- data in -----------------------------------------------------------------------------

    /// Installs a guild's roles (sorted) and drops what no longer exists from the editor.
    pub fn set_roles(&mut self, guild: u64, roles: Vec<Role>, version: Option<u64>) {
        self.data.roles.insert(guild, sorted(roles));
        if let Some(v) = version {
            self.data.role_versions.insert(guild, v);
        }
        let ids: Vec<u64> = self.roles_of(guild).iter().map(|r| r.id).collect();
        if let Some(ss) = self.ui.server_settings.as_mut().filter(|s| s.guild == guild) {
            if ss.form.as_ref().and_then(|f| f.role).is_some_and(|id| !ids.contains(&id)) {
                ss.form = None;
            }
            if ss.drag.is_some_and(|d| !ids.contains(&d.role)) {
                ss.drag = None;
            }
        }
    }

    /// Replaces a guild the server changed (name, icon) and fetches a new icon.
    pub fn set_guild(&mut self, g: Guild) {
        if let Some(slot) = self.data.guilds.iter_mut().find(|x| x.id == g.id) {
            *slot = g;
        }
        self.want_media();
    }
}

/// Whether the switch for `bit` shows on in `perms` (the raw bit; ADMINISTRATOR doesn't light the others).
pub fn has_bit(perms: Permissions, bit: Permissions) -> bool {
    perms.bits() & bit.bits() == bit.bits()
}
