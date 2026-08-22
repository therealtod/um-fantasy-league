//! Flyway's ordering rule, and nothing else of Flyway.
//!
//! The one behaviour that has to be reproduced exactly is that **Flyway orders
//! by version across locations, not location by location**. `db/` is split by
//! *kind* rather than by environment -- `db/migration` is schema and the
//! canonical hero/board catalogue, `db/seed` is the demo fixtures the `dev` and
//! `test` profiles add -- and the two interleave:
//!
//! ```text
//! migration V1, V2 | seed V3 | migration V4, V5 | seed V6 | migration V7
//!          | seed V8 | migration V9, V10
//! ```
//!
//! A runner that drained `migration/` and then `seed/` would try to insert the
//! demo fixtures before `V5__match_hero_pick.sql` had created the table they
//! fill, and fail on a foreign key long before it failed on anything
//! interesting. Hence: glob both, merge, sort by version once.
//!
//! Everything else Flyway does -- the `flyway_schema_history` table, checksum
//! validation, baselining, repair -- is deliberately absent. This runs against
//! a database created seconds ago and thrown away minutes later, so there is no
//! history to validate against.

use std::path::{Path, PathBuf};

use sqlx::{AssertSqlSafe, Executor, PgConnection};

/// One versioned SQL file, from either location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: u32,
    pub name: String,
    pub path: PathBuf,
}

/// The merged, version-ordered plan.
///
/// `seed_dir` is an `Option` because the seed location is a *profile* decision,
/// exactly as it is in `application.yml`: the base configuration migrates
/// `db/migration` alone and only `dev`/`test` add `db/seed`. A `None` here is
/// the `prod` database -- every hero and board, no league data at all.
pub fn plan(migration_dir: &Path, seed_dir: Option<&Path>) -> Vec<Migration> {
    let mut plan: Vec<Migration> = [Some(migration_dir), seed_dir]
        .into_iter()
        .flatten()
        .flat_map(scan)
        .collect();
    // `sort_by_key`, not `sort_unstable_by_key`: banned repo-wide by
    // `clippy.toml`. Versions are unique here, so it is only consistency.
    plan.sort_by_key(|m| m.version);

    if let Some(dup) = plan.windows(2).find(|w| w[0].version == w[1].version) {
        panic!("duplicate migration version: {:?} and {:?}", dup[0], dup[1]);
    }
    assert!(
        !plan.is_empty(),
        "no migrations under {}",
        migration_dir.display()
    );
    plan
}

/// `V(\d+)__<name>.sql`, by hand rather than by regex -- the format is fixed
/// and frozen, and a regex crate would be a dependency for one split.
fn scan(dir: &Path) -> Vec<Migration> {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.expect("directory entry").path();
        let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((version, name)) = file
            .strip_suffix(".sql")
            .and_then(|stem| stem.strip_prefix('V'))
            .and_then(|stem| stem.split_once("__"))
        else {
            continue;
        };
        let version = version
            .parse()
            .unwrap_or_else(|e| panic!("{file}: unparseable version `{version}`: {e}"));
        found.push(Migration {
            version,
            name: name.to_owned(),
            path,
        });
    }
    found
}

/// Applies the plan in order on one connection.
///
/// `raw_sql` uses the simple query protocol, so a whole file goes to the server
/// as a single implicit transaction -- which is what Flyway does for a
/// PostgreSQL migration, and what lets a file create a table and fill it.
pub async fn run(conn: &mut PgConnection, plan: &[Migration]) {
    for migration in plan {
        let sql = std::fs::read_to_string(&migration.path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", migration.path.display()));
        conn.execute(sqlx::raw_sql(AssertSqlSafe(sql)))
            .await
            .unwrap_or_else(|e| panic!("V{}__{} failed: {e}", migration.version, migration.name));
    }
}
