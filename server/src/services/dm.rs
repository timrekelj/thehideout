use std::time::Duration;

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use futures::future::try_join_all;
use scylla::statement::batch::{Batch, BatchType};

use crate::AppState;
use crate::access::find_user;
use crate::auth::{authenticate, internal};
use crate::convert::{datetime_to_proto, id_from_db, id_to_db, snowflake_datetime};
use crate::proto::thehideout::v1::*;

/// DM opens allowed per user within [`OPEN_DM_WINDOW`]. Opens are idempotent, so this only
/// bounds probing for which user IDs exist.
pub const OPEN_DM_MAX: usize = 20;
const OPEN_DM_WINDOW: Duration = Duration::from_secs(15 * 60);

pub struct DmSvc {
    state: AppState,
}

impl DmSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// Canonical pair key for two user IDs: ascending order, so (a, b) and (b, a) map to one key.
fn pair_key(a: u64, b: u64) -> String {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    format!("{}:{}", id_to_db(lo), id_to_db(hi))
}

impl DmService for DmSvc {
    async fn open_dm(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, OpenDmRequest>,
    ) -> ServiceResult<OpenDmResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        if req.user_id == user_id {
            return Err(ConnectError::invalid_argument(
                "cannot open a direct message with yourself",
            ));
        }
        self.state
            .limiter
            .check(&format!("open_dm:{user_id}"), OPEN_DM_MAX, OPEN_DM_WINDOW)?;
        // The target must be a real account; `NotFound` here is an existence oracle, bounded
        // by the per-user open rate limit above.
        let target = find_user(&self.state, id_to_db(req.user_id))
            .await?
            .filter(|u| !u.deleted)
            .ok_or_else(|| ConnectError::not_found("user not found"))?;
        let caller = find_user(&self.state, id_to_db(user_id))
            .await?
            .ok_or_else(|| ConnectError::unauthenticated("user no longer exists"))?;

        let (user_a, user_b) = if user_id < req.user_id {
            (user_id, req.user_id)
        } else {
            (req.user_id, user_id)
        };
        let pair = pair_key(user_a, user_b);
        let candidate = self.state.snowflake.next_id();
        let candidate_db = id_to_db(candidate);
        let created_at = snowflake_datetime(candidate);

        // Write the channel row first: if a concurrent open wins the pair LWT below, this row
        // is an unreachable orphan, which is harmless; the reverse order could leave the pair
        // pointing at a channel that was never written.
        self.state
            .session
            .execute_unpaged(
                "INSERT INTO dm_channels (id, user_a, user_b, created_at) VALUES (?, ?, ?, ?)",
                (candidate_db, id_to_db(user_a), id_to_db(user_b), created_at),
            )
            .await
            .map_err(internal)?;

        // An LWT returns `[applied]` plus the existing row's columns when not applied.
        let (applied, _, existing_dm_id) = self
            .state
            .session
            .execute_unpaged(
                "INSERT INTO dm_channels_by_pair (pair, dm_id) VALUES (?, ?) IF NOT EXISTS",
                (&pair, candidate_db),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(bool, Option<String>, Option<i64>)>()
            .map_err(internal)?;

        let channel = if applied {
            // Claim won: index the channel for both participants in one logged batch.
            let mut batch = Batch::new(BatchType::Logged);
            batch.append_statement(
                "INSERT INTO dms_by_user (user_id, dm_id, created_at) VALUES (?, ?, ?)",
            );
            batch.append_statement(
                "INSERT INTO dms_by_user (user_id, dm_id, created_at) VALUES (?, ?, ?)",
            );
            self.state
                .session
                .batch(
                    &batch,
                    (
                        (id_to_db(user_a), candidate_db, created_at),
                        (id_to_db(user_b), candidate_db, created_at),
                    ),
                )
                .await
                .map_err(internal)?;
            // Tell both users' open streams about the new channel so they start listening to it.
            let channel = dm_to_proto(candidate, caller.clone(), target.clone(), created_at);
            self.state
                .events
                .notify_user_opened_dm(user_a, channel.clone());
            self.state
                .events
                .notify_user_opened_dm(user_b, channel.clone());
            tracing::info!(dm_id = candidate, user_a, user_b, "dm channel opened");
            channel
        } else {
            // A channel for this pair already exists; return it.
            let existing = existing_dm_id
                .ok_or_else(|| internal("pair LWT did not return the existing dm_id"))?;
            self.load_dm(existing)
                .await?
                .ok_or_else(|| internal("dm channel missing for existing pair"))?
        };

        Response::ok(OpenDmResponse {
            channel: channel.into(),
            ..Default::default()
        })
    }

    async fn list_dms(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, ListDmsRequest>,
    ) -> ServiceResult<ListDmsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let dm_ids = self
            .state
            .session
            .execute_unpaged(
                "SELECT dm_id FROM dms_by_user WHERE user_id = ?",
                (id_to_db(user_id),),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64,)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;

        let mut channels = try_join_all(dm_ids.into_iter().map(|(id,)| self.load_dm(id)))
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        channels.sort_by_key(|c| std::cmp::Reverse(c.id));
        Response::ok(ListDmsResponse {
            channels,
            ..Default::default()
        })
    }
}

impl DmSvc {
    /// Loads a DM channel with both participants; `None` if the row is gone.
    async fn load_dm(&self, db_id: i64) -> Result<Option<DmChannel>, ConnectError> {
        let Some((user_a, user_b, created_at)) = self
            .state
            .session
            .execute_unpaged(
                "SELECT user_a, user_b, created_at FROM dm_channels WHERE id = ?",
                (db_id,),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64, i64, DateTime<Utc>)>()
            .map_err(internal)?
        else {
            return Ok(None);
        };
        let user_a = find_user(&self.state, user_a).await?;
        let user_b = find_user(&self.state, user_b).await?;
        // user_a < user_b by construction, so participants come out ordered by user ID.
        let participants = [user_a, user_b].into_iter().flatten().collect();
        Ok(Some(DmChannel {
            id: id_from_db(db_id),
            participants,
            created_at: datetime_to_proto(created_at).into(),
            ..Default::default()
        }))
    }
}

/// Builds the proto for a freshly created DM channel; participants ordered by user ID.
fn dm_to_proto(id: u64, user_a: User, user_b: User, created_at: DateTime<Utc>) -> DmChannel {
    let (first, second) = if user_a.id <= user_b.id {
        (user_a, user_b)
    } else {
        (user_b, user_a)
    };
    DmChannel {
        id,
        participants: vec![first, second],
        created_at: datetime_to_proto(created_at).into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_key_is_order_independent() {
        assert_eq!(pair_key(5, 9), pair_key(9, 5));
        assert_eq!(pair_key(5, 9), format!("{}:{}", id_to_db(5), id_to_db(9)));
        assert_ne!(pair_key(5, 9), pair_key(5, 10));
    }
}
