use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use scylla::client::session::Session;
use scylla::errors::{DbError, ExecutionError, RequestAttemptError};

/// Embedded migrations, in version order.
pub const MIGRATIONS: &[(i32, &str, &str)] = &[
    (
        1,
        "init",
        include_str!("../migrations/0001_init.cql"),
    ),
    (
        2,
        "quotas",
        include_str!("../migrations/0002_quotas.cql"),
    ),
    (
        3,
        "dms",
        include_str!("../migrations/0003_dms.cql"),
    ),
    (
        4,
        "roles_layout",
        include_str!("../migrations/0004_roles_layout.cql"),
    ),
    (
        5,
        "media",
        include_str!("../migrations/0005_media.cql"),
    ),
    (
        6,
        "emojis_reactions",
        include_str!("../migrations/0006_emojis_reactions.cql"),
    ),
    (
        7,
        "attachments",
        include_str!("../migrations/0007_attachments.cql"),
    ),
    (
        8,
        "embeds",
        include_str!("../migrations/0008_embeds.cql"),
    ),
    (
        9,
        "embed_media",
        include_str!("../migrations/0009_embed_media.cql"),
    ),
    (
        10,
        "admin",
        include_str!("../migrations/0010_admin.cql"),
    ),
    (
        11,
        "auth",
        include_str!("../migrations/0011_auth.cql"),
    ),
    (
        12,
        "voice",
        include_str!("../migrations/0012_voice.cql"),
    ),
    (
        13,
        "moderation",
        include_str!("../migrations/0013_moderation.cql"),
    ),
    (
        14,
        "account_lifecycle",
        include_str!("../migrations/0014_account_lifecycle.cql"),
    ),
];

/// How the keyspace is replicated (`THEHIDEOUT_SCYLLA_REPLICATION`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replication {
    /// `SimpleStrategy:<rf>`
    Simple(u32),
    /// `NetworkTopologyStrategy:dc1=3,dc2=3`
    NetworkTopology(Vec<(String, u32)>),
}

impl Default for Replication {
    fn default() -> Self {
        Self::Simple(1)
    }
}

impl Replication {
    /// `SimpleStrategy:1` or `NetworkTopologyStrategy:dc1=3,dc2=3`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let usage = "expected SimpleStrategy:<n> or NetworkTopologyStrategy:<dc>=<n>[,<dc>=<n>...]";
        let (class, rest) = s.split_once(':').with_context(|| format!("invalid THEHIDEOUT_SCYLLA_REPLICATION {s:?}, {usage}"))?;
        let factor = |n: &str| -> Result<u32> {
            let n: u32 = n.trim().parse().with_context(|| format!("invalid replication factor {n:?} in THEHIDEOUT_SCYLLA_REPLICATION"))?;
            anyhow::ensure!(n >= 1, "replication factors in THEHIDEOUT_SCYLLA_REPLICATION must be at least 1");
            Ok(n)
        };
        match class.trim() {
            "SimpleStrategy" => Ok(Self::Simple(factor(rest)?)),
            "NetworkTopologyStrategy" => {
                let mut dcs = Vec::new();
                for item in rest.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                    let (dc, n) = item.split_once('=').with_context(|| format!("invalid datacenter {item:?} in THEHIDEOUT_SCYLLA_REPLICATION, {usage}"))?;
                    let dc = dc.trim();
                    anyhow::ensure!(
                        !dc.is_empty() && dc.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
                        "invalid datacenter name {dc:?} in THEHIDEOUT_SCYLLA_REPLICATION"
                    );
                    anyhow::ensure!(!dcs.iter().any(|(d, _)| d == dc), "datacenter {dc:?} is listed twice in THEHIDEOUT_SCYLLA_REPLICATION");
                    dcs.push((dc.to_owned(), factor(n)?));
                }
                anyhow::ensure!(!dcs.is_empty(), "NetworkTopologyStrategy in THEHIDEOUT_SCYLLA_REPLICATION needs at least one <dc>=<n>");
                Ok(Self::NetworkTopology(dcs))
            }
            other => bail!("unknown replication strategy {other:?} in THEHIDEOUT_SCYLLA_REPLICATION, {usage}"),
        }
    }

    /// The CQL replication map, e.g. `{'class': 'SimpleStrategy', 'replication_factor': 1}`.
    pub fn cql(&self) -> String {
        match self {
            Self::Simple(rf) => format!("{{'class': 'SimpleStrategy', 'replication_factor': {rf}}}"),
            Self::NetworkTopology(dcs) => {
                let dcs: String = dcs.iter().map(|(dc, n)| format!(", '{dc}': {n}")).collect();
                format!("{{'class': 'NetworkTopologyStrategy'{dcs}}}")
            }
        }
    }

    /// Whether a keyspace's `system_schema.keyspaces.replication` map is this replication.
    pub fn matches(&self, actual: &HashMap<String, String>) -> bool {
        let class = actual.get("class").map_or("", |c| c.rsplit('.').next().unwrap_or(c));
        let others: HashMap<&str, &str> = actual.iter().filter(|(k, _)| *k != "class").map(|(k, v)| (k.as_str(), v.as_str())).collect();
        match self {
            Self::Simple(rf) => class == "SimpleStrategy" && others.get("replication_factor").is_some_and(|v| v.parse() == Ok(*rf)),
            Self::NetworkTopology(dcs) => {
                // Newer servers may add `replication_factor`-free extras; compare the datacenters only.
                class == "NetworkTopologyStrategy"
                    && others.len() == dcs.len()
                    && dcs.iter().all(|(dc, n)| others.get(dc.as_str()).is_some_and(|v| v.parse() == Ok(*n)))
            }
        }
    }
}

/// How long a held migration lock lives without a refresh.
const LOCK_TTL_SECS: i32 = 300;
/// How long to wait for another instance's migrations before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(600);
const LOCK_POLL: Duration = Duration::from_secs(2);

/// The `[applied]` column of a lightweight-transaction result.
fn applied(result: scylla::response::query_result::QueryResult) -> Result<bool> {
    let row = result.into_rows_result()?.first_row::<scylla::value::Row>()?;
    Ok(row.columns.first().and_then(|c| c.as_ref()).and_then(|v| v.as_boolean()).unwrap_or(false))
}

/// Creates the keyspace if needed (with the default replication), selects it
/// on the session, and applies any migrations not yet recorded.
pub async fn run(session: &Session, keyspace: &str) -> Result<()> {
    run_with(session, keyspace, &Replication::default()).await
}

/// Like [`run`], creating a missing keyspace with `replication`.
///
/// Instances starting together take turns: the migrations run under a lock row
/// (`schema_lock`, an `INSERT ... IF NOT EXISTS USING TTL` lightweight
/// transaction, refreshed between migrations and released at the end; a crashed
/// holder's lock expires after [`LOCK_TTL_SECS`]). A version is recorded in
/// `schema_migrations` only after all of its statements succeeded, and a
/// statement that fails with "already exists" (left behind by an earlier
/// interrupted run of the same version) is skipped, so a half-applied version is
/// completed on the next start.
pub async fn run_with(session: &Session, keyspace: &str, replication: &Replication) -> Result<()> {
    anyhow::ensure!(
        !keyspace.is_empty() && keyspace.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "invalid keyspace name {keyspace:?}"
    );
    session
        .query_unpaged(format!("CREATE KEYSPACE IF NOT EXISTS {keyspace} WITH replication = {}", replication.cql()), ())
        .await
        .context("creating keyspace")?;
    warn_if_replication_differs(session, keyspace, replication).await;
    session
        .use_keyspace(keyspace, false)
        .await
        .context("selecting keyspace")?;
    session
        .query_unpaged(
            format!(
                "CREATE TABLE IF NOT EXISTS {keyspace}.schema_migrations \
                 (version int PRIMARY KEY, name text, applied_at timestamp)"
            ),
            (),
        )
        .await
        .context("creating schema_migrations")?;
    session
        .query_unpaged(format!("CREATE TABLE IF NOT EXISTS {keyspace}.schema_lock (name text PRIMARY KEY, owner text)"), ())
        .await
        .context("creating schema_lock")?;

    let owner = format!("{:032x}", rand::random::<u128>());
    acquire_lock(session, keyspace, &owner).await?;
    let result = apply(session, keyspace, &owner).await;
    // Release even after a failure so the next start does not wait out the TTL.
    if let Err(e) = session
        .query_unpaged(format!("DELETE FROM {keyspace}.schema_lock WHERE name = 'migrate' IF owner = ?"), (&owner,))
        .await
    {
        tracing::warn!(error = %e, "could not release the migration lock; it expires on its own");
    }
    result
}

async fn acquire_lock(session: &Session, keyspace: &str, owner: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + LOCK_WAIT;
    let mut announced = false;
    loop {
        let won = session
            .query_unpaged(
                format!("INSERT INTO {keyspace}.schema_lock (name, owner) VALUES ('migrate', ?) IF NOT EXISTS USING TTL {LOCK_TTL_SECS}"),
                (owner,),
            )
            .await
            .context("taking the migration lock")?;
        if applied(won)? {
            return Ok(());
        }
        if !announced {
            tracing::info!("another instance is running migrations; waiting for it");
            announced = true;
        }
        anyhow::ensure!(tokio::time::Instant::now() < deadline, "timed out waiting for another instance's migrations (lock {keyspace}.schema_lock)");
        tokio::time::sleep(LOCK_POLL).await;
    }
}

async fn warn_if_replication_differs(session: &Session, keyspace: &str, wanted: &Replication) {
    let actual = async {
        session
            .query_unpaged("SELECT replication FROM system_schema.keyspaces WHERE keyspace_name = ?", (keyspace,))
            .await?
            .into_rows_result()?
            .maybe_first_row::<(HashMap<String, String>,)>()
            .map_err(anyhow::Error::from)
    }
    .await;
    match actual {
        Ok(Some((actual,))) if !wanted.matches(&actual) => {
            tracing::warn!(
                keyspace,
                existing = ?actual,
                configured = %wanted.cql(),
                "the keyspace exists with a different replication than THEHIDEOUT_SCYLLA_REPLICATION; \
                 leaving it unchanged (ALTER KEYSPACE and repair manually to change it)"
            );
        }
        Ok(_) => {}
        Err(e) => tracing::debug!(error = %e, "could not read the keyspace replication"),
    }
}

/// Applies the unrecorded migrations; the caller holds the lock.
async fn apply(session: &Session, keyspace: &str, owner: &str) -> Result<()> {
    // Read after taking the lock: another instance may just have applied some.
    let applied: HashSet<i32> = session
        .query_unpaged(
            format!("SELECT version FROM {keyspace}.schema_migrations"),
            (),
        )
        .await?
        .into_rows_result()?
        .rows::<(i32,)>()?
        .map(|r| r.map(|(v,)| v))
        .collect::<Result<_, _>>()?;

    for &(version, name, cql) in MIGRATIONS {
        if applied.contains(&version) {
            tracing::info!(version, name, "migration already applied, skipping");
            continue;
        }
        // Keep the lock alive through a long run of migrations.
        session
            .query_unpaged(
                format!("UPDATE {keyspace}.schema_lock USING TTL {LOCK_TTL_SECS} SET owner = ? WHERE name = 'migrate' IF owner = ?"),
                (owner, owner),
            )
            .await
            .context("refreshing the migration lock")?;
        for stmt in statements(cql) {
            match session.query_unpaged(stmt.clone(), ()).await {
                Ok(_) => {}
                Err(ExecutionError::LastAttemptError(RequestAttemptError::DbError(DbError::AlreadyExists { .. }, _))) => {
                    tracing::warn!(version, name, "object already exists (an earlier run was interrupted); continuing");
                }
                Err(e) => return Err(e).with_context(|| format!("migration {version:04}_{name}: {stmt}")),
            }
        }
        // Only now, with every statement done, is the version recorded.
        session
            .query_unpaged(
                format!(
                    "INSERT INTO {keyspace}.schema_migrations (version, name, applied_at) \
                     VALUES (?, ?, toTimestamp(now()))"
                ),
                (version, name),
            )
            .await?;
        tracing::info!(version, name, "applied migration");
    }
    Ok(())
}

/// Splits a CQL file into statements. Statements are unqualified; `run` selects
/// the keyspace on the session first.
fn statements(cql: &str) -> Vec<String> {
    let without_comments: String = cql
        .lines()
        .map(|l| l.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    without_comments
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_statements_and_strips_comments() {
        let s = statements("-- hi\nCREATE TABLE a (x int); -- c\nCREATE TABLE b (y int);\n");
        assert_eq!(s, vec!["CREATE TABLE a (x int)", "CREATE TABLE b (y int)"]);
    }

    #[test]
    fn migrations_are_ordered() {
        assert!(MIGRATIONS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn replication_parses_and_renders() {
        assert_eq!(Replication::parse("SimpleStrategy:1").unwrap(), Replication::Simple(1));
        assert_eq!(Replication::default().cql(), "{'class': 'SimpleStrategy', 'replication_factor': 1}");
        assert_eq!(Replication::parse(" SimpleStrategy: 3 ").unwrap(), Replication::Simple(3));
        let nts = Replication::parse("NetworkTopologyStrategy:dc1=3, dc2=2").unwrap();
        assert_eq!(nts, Replication::NetworkTopology(vec![("dc1".into(), 3), ("dc2".into(), 2)]));
        assert_eq!(nts.cql(), "{'class': 'NetworkTopologyStrategy', 'dc1': 3, 'dc2': 2}");
        assert!(Replication::parse("NetworkTopologyStrategy:eu-west.1=3").is_ok());
    }

    #[test]
    fn bad_replication_is_rejected() {
        for bad in [
            "",
            "SimpleStrategy",
            "SimpleStrategy:0",
            "SimpleStrategy:x",
            "SimpleStrategy:dc1=3",
            "NetworkTopologyStrategy:",
            "NetworkTopologyStrategy:dc1",
            "NetworkTopologyStrategy:dc1=0",
            "NetworkTopologyStrategy:dc1=3,dc1=2",
            "NetworkTopologyStrategy:d'c=3",
            "LocalStrategy:1",
        ] {
            assert!(Replication::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn existing_replication_is_compared_with_the_configured_one() {
        let simple = map(&[("class", "org.apache.cassandra.locator.SimpleStrategy"), ("replication_factor", "1")]);
        assert!(Replication::Simple(1).matches(&simple));
        assert!(!Replication::Simple(3).matches(&simple));
        let nts = map(&[("class", "org.apache.cassandra.locator.NetworkTopologyStrategy"), ("dc1", "3"), ("dc2", "3")]);
        let want = Replication::parse("NetworkTopologyStrategy:dc2=3,dc1=3").unwrap();
        assert!(want.matches(&nts));
        assert!(!Replication::parse("NetworkTopologyStrategy:dc1=3").unwrap().matches(&nts));
        assert!(!Replication::parse("NetworkTopologyStrategy:dc1=3,dc2=2").unwrap().matches(&nts));
        assert!(!Replication::Simple(1).matches(&nts));
        assert!(!want.matches(&simple));
    }
}
