//! Roles, media and site-admin calls of [`Client`].

use bytes::Bytes;
use connectrpc::client::{ClientTransport, full_body};

use crate::transport::Transport;
use http_body_util::BodyExt;

use crate::admin::{
    ActivityPoint, AdminFileKind, AdminFileRow, AdminGuildRow, AdminUserRow, AuditEntry, Overview, Paged, next_token,
};
use crate::client::{Client, MAX_MEDIA_BYTES, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::structure::{Member, PaletteColor, Permissions, Role, RoleList};
use crate::types::{Guild, User};

type Result<T> = std::result::Result<T, ApiError>;

impl Client {
    fn roles(&self) -> Result<pb::RoleServiceClient<Transport>> {
        Ok(pb::RoleServiceClient::new(self.http.clone(), self.config()?))
    }

    fn media(&self) -> Result<pb::MediaServiceClient<Transport>> {
        Ok(pb::MediaServiceClient::new(self.http.clone(), self.config()?))
    }

    pub(crate) fn admin(&self) -> Result<pb::AdminServiceClient<Transport>> {
        Ok(pb::AdminServiceClient::new(self.http.clone(), self.config()?))
    }

    // ---- roles -----------------------------------------------------------

    /// The roles of a guild, most senior first, with the version for [`reorder_roles`](Self::reorder_roles).
    pub async fn list_roles(&self, guild_id: u64) -> Result<RoleList> {
        let res = self.roles()?.list_roles(pb::ListRolesRequest { guild_id, ..Default::default() }).await?.into_owned();
        Ok(RoleList { roles: res.roles.into_iter().map(Into::into).collect(), version: res.version })
    }

    /// Creates a role at the bottom of the hierarchy (needs `MANAGE_ROLES`).
    /// Returns it with the new roles version.
    pub async fn create_role(
        &self,
        guild_id: u64,
        name: &str,
        color: PaletteColor,
        permissions: Permissions,
    ) -> Result<(Role, u64)> {
        let res = self
            .roles()?
            .create_role(pb::CreateRoleRequest {
                guild_id,
                name: name.into(),
                color: color.as_key().into(),
                permissions: permissions.bits(),
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok((required(res.role.into_option())?, res.version))
    }

    /// Updates a role; `None` fields stay unchanged.
    pub async fn update_role(
        &self,
        guild_id: u64,
        role_id: u64,
        name: Option<&str>,
        color: Option<PaletteColor>,
        permissions: Option<Permissions>,
    ) -> Result<Role> {
        let res = self
            .roles()?
            .update_role(pb::UpdateRoleRequest {
                guild_id,
                role_id,
                name: name.map(str::to_owned),
                color: color.map(|c| c.as_key().to_owned()),
                permissions: permissions.map(Permissions::bits),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.role.into_option())
    }

    /// Deletes a role and removes it from all members. Returns the new roles version.
    pub async fn delete_role(&self, guild_id: u64, role_id: u64) -> Result<u64> {
        let res = self
            .roles()?
            .delete_role(pb::DeleteRoleRequest { guild_id, role_id, ..Default::default() })
            .await?
            .into_owned();
        Ok(res.version)
    }

    /// Sets the full role order (most senior first), based on `expected_version`.
    /// A stale version fails with [`ApiError::Conflict`].
    pub async fn reorder_roles(&self, guild_id: u64, expected_version: u64, role_ids: &[u64]) -> Result<RoleList> {
        let res = self
            .roles()?
            .reorder_roles(pb::ReorderRolesRequest {
                guild_id,
                expected_version,
                role_ids: role_ids.to_vec(),
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(RoleList { roles: res.roles.into_iter().map(Into::into).collect(), version: res.version })
    }

    /// Replaces the set of roles assigned to a member.
    pub async fn set_member_roles(&self, guild_id: u64, user_id: u64, role_ids: &[u64]) -> Result<Member> {
        let res = self
            .roles()?
            .set_member_roles(pb::SetMemberRolesRequest { guild_id, user_id, role_ids: role_ids.to_vec(), ..Default::default() })
            .await?
            .into_owned();
        required(res.member.into_option())
    }

    // ---- media -----------------------------------------------------------

    /// URL of a stored image (`User::avatar` / `Guild::icon` key). Plain HTTP GET, no auth needed.
    pub fn media_url(&self, key: &str) -> String {
        format!("{}/media/{key}", self.base_url.trim_end_matches('/'))
    }

    /// Downloads a stored image. [`ApiError::NotFound`] for an unknown key.
    pub async fn fetch_media(&self, key: &str) -> Result<Vec<u8>> {
        let uri: http::Uri = self
            .media_url(key)
            .parse()
            .map_err(|e| ApiError::InvalidArgument(format!("bad media url: {e}")))?;
        let request = http::Request::get(uri)
            .body(full_body(Bytes::new()))
            .map_err(|e| ApiError::Other(e.to_string()))?;
        let response = self.http.send(request).await?;
        let status = response.status();
        match status {
            s if s.is_success() => {}
            http::StatusCode::NOT_FOUND => return Err(ApiError::NotFound),
            s => return Err(ApiError::Other(format!("media request failed: {s}"))),
        }
        let body = response.into_body().collect().await.map_err(|e| ApiError::Network(e.to_string()))?;
        Ok(body.to_bytes().to_vec())
    }

    /// Sets the caller's avatar (PNG/JPEG/WebP/GIF, at most [`MAX_MEDIA_BYTES`]).
    pub async fn upload_avatar(&self, data: Vec<u8>, content_type: &str) -> Result<User> {
        check_size(&data)?;
        let res = self
            .media()?
            .upload_avatar(pb::UploadAvatarRequest { data, content_type: content_type.into(), ..Default::default() })
            .await?
            .into_owned();
        required(res.user.into_option())
    }

    /// Removes the caller's avatar.
    pub async fn remove_avatar(&self) -> Result<User> {
        let res = self.media()?.remove_avatar(pb::RemoveAvatarRequest::default()).await?.into_owned();
        required(res.user.into_option())
    }

    /// Sets a guild's icon (needs `MANAGE_SERVER`; same limits as [`upload_avatar`](Self::upload_avatar)).
    pub async fn upload_guild_icon(&self, guild_id: u64, data: Vec<u8>, content_type: &str) -> Result<Guild> {
        check_size(&data)?;
        let res = self
            .media()?
            .upload_guild_icon(pb::UploadGuildIconRequest {
                guild_id,
                data,
                content_type: content_type.into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.guild.into_option())
    }

    /// Removes a guild's icon (needs `MANAGE_SERVER`).
    pub async fn remove_guild_icon(&self, guild_id: u64) -> Result<Guild> {
        let res = self
            .media()?
            .remove_guild_icon(pb::RemoveGuildIconRequest { guild_id, ..Default::default() })
            .await?
            .into_owned();
        required(res.guild.into_option())
    }

    // ---- site admin (needs `User::is_admin`) -----------------------------

    /// Site-wide totals.
    pub async fn admin_overview(&self) -> Result<Overview> {
        Ok(self.admin()?.get_overview(pb::GetOverviewRequest::default()).await?.into_owned().into())
    }

    /// Per-day activity for the last `days` days (1..=90, 0 = server default), oldest first.
    pub async fn admin_activity(&self, days: u32) -> Result<Vec<ActivityPoint>> {
        let res = self.admin()?.get_activity(pb::GetActivityRequest { days, ..Default::default() }).await?.into_owned();
        Ok(res.points.into_iter().map(Into::into).collect())
    }

    /// Lists all guilds with usage numbers. `limit` 0 = server default.
    pub async fn admin_list_guilds(
        &self,
        query: &str,
        page_token: Option<&str>,
        limit: u32,
    ) -> Result<Paged<AdminGuildRow>> {
        let res = self
            .admin()?
            .list_guilds(pb::ListGuildsRequest {
                page_token: page_token.unwrap_or_default().into(),
                limit,
                query: query.into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(Paged {
            items: res.guilds.into_iter().filter_map(AdminGuildRow::from_proto).collect(),
            next_page_token: next_token(res.next_page_token),
        })
    }

    /// Lists all users with usage numbers. `limit` 0 = server default.
    pub async fn admin_list_users(
        &self,
        query: &str,
        page_token: Option<&str>,
        limit: u32,
    ) -> Result<Paged<AdminUserRow>> {
        let res = self
            .admin()?
            .list_users(pb::ListUsersRequest {
                page_token: page_token.unwrap_or_default().into(),
                limit,
                query: query.into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(Paged {
            items: res.users.into_iter().filter_map(AdminUserRow::from_proto).collect(),
            next_page_token: next_token(res.next_page_token),
        })
    }

    /// Grants or revokes site-admin rights. Demoting yourself or the last admin
    /// fails with [`ApiError::Precondition`].
    pub async fn admin_set_admin(&self, user_id: u64, is_admin: bool) -> Result<User> {
        let res = self.admin()?.set_admin(pb::SetAdminRequest { user_id, is_admin, ..Default::default() }).await?.into_owned();
        required(res.user.into_option())
    }

    /// Lists stored files, largest first, optionally of one guild and/or kind.
    pub async fn admin_list_files(
        &self,
        guild_id: Option<u64>,
        kind: Option<AdminFileKind>,
        page_token: Option<&str>,
        limit: u32,
    ) -> Result<Paged<AdminFileRow>> {
        let res = self
            .admin()?
            .list_files(pb::ListFilesRequest {
                guild_id,
                kind: kind.and_then(AdminFileKind::to_proto).map(Into::into),
                page_token: page_token.unwrap_or_default().into(),
                limit,
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(Paged {
            items: res.files.into_iter().map(Into::into).collect(),
            next_page_token: next_token(res.next_page_token),
        })
    }

    /// Deletes a stored file and its owner reference; `reason` goes to the audit log.
    pub async fn admin_delete_file(&self, key: &str, reason: &str) -> Result<()> {
        self.admin()?.delete_file(pb::DeleteFileRequest { key: key.into(), reason: reason.into(), ..Default::default() }).await?;
        Ok(())
    }

    /// Lists audit entries, newest first.
    pub async fn admin_list_audit_log(&self, page_token: Option<&str>, limit: u32) -> Result<Paged<AuditEntry>> {
        let res = self
            .admin()?
            .list_audit_log(pb::ListAuditLogRequest { page_token: page_token.unwrap_or_default().into(), limit, ..Default::default() })
            .await?
            .into_owned();
        Ok(Paged {
            items: res.entries.into_iter().map(Into::into).collect(),
            next_page_token: next_token(res.next_page_token),
        })
    }
}

fn check_size(data: &[u8]) -> Result<()> {
    if data.len() > MAX_MEDIA_BYTES {
        return Err(ApiError::InvalidArgument(format!(
            "image is {} bytes, the limit is {MAX_MEDIA_BYTES}",
            data.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_url_joins_base_and_key() {
        assert_eq!(Client::new("http://h:1").media_url("abc"), "http://h:1/media/abc");
        assert_eq!(Client::new("http://h:1/").media_url("abc"), "http://h:1/media/abc");
    }

    #[test]
    fn size_limit_is_8_mib_inclusive() {
        assert!(check_size(&vec![0; MAX_MEDIA_BYTES]).is_ok());
        assert!(matches!(check_size(&vec![0; MAX_MEDIA_BYTES + 1]), Err(ApiError::InvalidArgument(_))));
    }

    #[tokio::test]
    async fn oversized_uploads_fail_locally() {
        // Unroutable port: reaching the network would yield `Network`, not `InvalidArgument`.
        let client = Client::new("http://127.0.0.1:1");
        let big = vec![0u8; MAX_MEDIA_BYTES + 1];
        assert!(matches!(client.upload_avatar(big.clone(), "image/png").await, Err(ApiError::InvalidArgument(_))));
        assert!(matches!(client.upload_guild_icon(1, big, "image/png").await, Err(ApiError::InvalidArgument(_))));
    }
}
