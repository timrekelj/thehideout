use std::collections::HashSet;

use anyhow::{Context, Result};
use scylla::client::session::Session;

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
];

/// Creates the keyspace if needed, selects it on the session, and applies any
/// migrations not yet recorded.
pub async fn run(session: &Session, keyspace: &str) -> Result<()> {
    anyhow::ensure!(
        !keyspace.is_empty() && keyspace.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "invalid keyspace name {keyspace:?}"
    );
    session
        .query_unpaged(
            format!(
                "CREATE KEYSPACE IF NOT EXISTS {keyspace} WITH replication = \
                 {{'class': 'SimpleStrategy', 'replication_factor': 1}}"
            ),
            (),
        )
        .await
        .context("creating keyspace")?;
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
        for stmt in statements(cql) {
            session
                .query_unpaged(stmt.clone(), ())
                .await
                .with_context(|| format!("migration {version:04}_{name}: {stmt}"))?;
        }
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
}
