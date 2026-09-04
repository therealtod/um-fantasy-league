//! Migrates a real PostgreSQL through every file in `db/`, then asserts the
//! shape of what came out. A green run here is the proof that the schema and
//! the seed are valid, not merely well-formed -- and that the runner in
//! `harness::migrate` interleaved the two Flyway locations by version the way
//! Flyway does: a migration and a seed file both add a table the other
//! depends on, and getting the version ordering wrong across locations would
//! surface here as a migration failure rather than a passing run against the
//! wrong schema.
//!
//! **The counts are exact, and stay exact.** The seed is a fixture the scoring
//! and standings tests assert against *by value*: the leaderboard parity
//! check in `tests/it/standings.rs` (ArthurianLegend 100.00, NeonStrategist
//! 79.75, ...) is arithmetic over these rows. A row quietly appearing or disappearing has to
//! fail here, by name, rather than three tests later as an unexplained
//! arithmetic mismatch. Softening one of these into `>= 1` would delete the
//! only thing that makes those numbers mean anything.
//!
//! These assertions ask the database directly rather than going through a
//! read module: the read modules are other tests' subject matter, and here
//! they would only stand between the assertion and the rows it is about.

use sqlx::{PgPool, Row};

use crate::harness::TestApp;

async fn count(pool: &PgPool, table: &str, filter: &str) -> i64 {
    // `table`/`filter` are literals from this file, never input.
    let sql = format!("select count(*) from {table} where {filter}");
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("counting {table}: {e}"))
}

async fn scalar(pool: &PgPool, sql: String) -> i64 {
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .expect("scalar query")
}

#[tokio::test]
async fn every_table_is_seeded_to_its_expected_size() {
    let app = TestApp::spawn().await;
    let db = app.pool();

    // These two come from `db/migration/V2__reference_data.sql`, not from the
    // fixtures: the hero and board catalogue migrates in every profile, so a
    // `prod` database carries these same counts with everything below at zero.
    assert_eq!(74, count(db, "heroes", "true").await);
    assert_eq!(35, count(db, "game_maps", "true").await);
    assert_eq!(4, count(db, "managers", "true").await);
    assert_eq!(3, count(db, "tournaments", "true").await);
    assert_eq!(
        32,
        count(db, "tournament_heroes", "true").await,
        "12 + 12 + Spring's narrower 8"
    );
    assert_eq!(7, count(db, "tournament_maps", "true").await);
    assert_eq!(4, count(db, "tournament_entries", "true").await);
    assert_eq!(
        12,
        count(db, "entry_slots", "true").await,
        "4 entries x roster size 3"
    );
    assert_eq!(3, count(db, "scoring_rule_sets", "true").await);
    assert_eq!(
        24,
        count(db, "scoring_coefficients", "true").await,
        "3 rule sets x 8 metrics"
    );
    assert_eq!(
        13,
        count(db, "tournament_matches", "true").await,
        "12 single-game matches + the Bo3 decider"
    );
    assert_eq!(
        26,
        count(db, "match_participants", "true").await,
        "13 matches x 2 sides"
    );
    assert_eq!(
        15,
        count(db, "match_games", "true").await,
        "12 single-game matches + the Bo3's 3 games"
    );
    assert_eq!(
        30,
        count(db, "match_game_participants", "true").await,
        "15 games x 2 sides"
    );
    assert_eq!(
        22,
        count(db, "hero_bans", "true").await,
        "19 original + the Bo3's one ban per category"
    );
    assert_eq!(
        28,
        count(db, "match_hero_picks", "true").await,
        "26 heroes fielded (12 matches x 2 sides, plus the Bo3's one hero per side across its \
         three games) + the Bo3's 2 drafted-and-never-fielded picks"
    );
}

#[tokio::test]
async fn the_three_tournaments_cover_the_lifecycle_states_the_lobby_renders() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select name, status, format, roster_size, credit_grant
         from tournaments order by start_date",
    )
    .fetch_all(app.pool())
    .await
    .expect("tournaments");

    let names: Vec<String> = rows.iter().map(|r| r.get("name")).collect();
    assert_eq!(
        vec![
            "Summer of Legends",
            "Winter of Champions",
            "Spring of Myths"
        ],
        names
    );

    let statuses: Vec<String> = rows.iter().map(|r| r.get("status")).collect();
    assert_eq!(
        vec!["COMPLETED", "REGISTRATION_OPEN", "SCHEDULED"],
        statuses
    );

    let formats: Vec<String> = rows.iter().map(|r| r.get("format")).collect();
    assert_eq!(vec!["BANQUEST", "ARSENAL", "BANQUEST"], formats);

    assert!(
        rows.iter()
            .all(|r| r.get::<i32, _>("roster_size") == 3
                && r.get::<i32, _>("credit_grant") == 10_000)
    );
}

#[tokio::test]
async fn dates_are_calendar_days_and_only_the_finished_tournament_has_an_end() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select name, start_date::text as start_date, end_date::text as end_date
         from tournaments where name in ('Summer of Legends', 'Winter of Champions')
         order by name",
    )
    .fetch_all(app.pool())
    .await
    .expect("tournaments");

    // Ordered by name: Summer, then Winter.
    assert_eq!("Summer of Legends", rows[0].get::<String, _>("name"));
    assert_eq!("2026-06-05", rows[0].get::<String, _>("start_date"));
    assert_eq!(
        "2026-06-07",
        rows[0].get::<Option<String>, _>("end_date").unwrap()
    );

    assert_eq!("Winter of Champions", rows[1].get::<String, _>("name"));
    assert_eq!("2026-08-14", rows[1].get::<String, _>("start_date"));
    assert_eq!(
        None,
        rows[1].get::<Option<String>, _>("end_date"),
        "an unfinished tournament has no end date"
    );
}

#[tokio::test]
async fn hero_cost_is_per_tournament_not_global() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select t.name as tournament, th.cost
         from tournament_heroes th
             join tournaments t on t.id = th.tournament_id
             join heroes h on h.id = th.hero_id
         where h.name = 'Sun Wukong'
         order by t.name",
    )
    .fetch_all(app.pool())
    .await
    .expect("costs");

    let costs: Vec<(String, i32)> = rows
        .iter()
        .map(|r| (r.get("tournament"), r.get("cost")))
        .collect();
    assert_eq!(
        vec![
            ("Spring of Myths".to_owned(), 5500),
            ("Summer of Legends".to_owned(), 5600),
            ("Winter of Champions".to_owned(), 5300),
        ],
        costs
    );
}

#[tokio::test]
async fn spring_of_myths_carries_a_narrower_pool_so_unknown_hero_is_reachable() {
    let app = TestApp::spawn().await;
    let spring_hero_count = scalar(
        app.pool(),
        "select count(*) from tournament_heroes th
             join tournaments t on t.id = th.tournament_id
         where t.name = 'Spring of Myths'"
            .to_owned(),
    )
    .await;

    assert_eq!(8, spring_hero_count);
}

#[tokio::test]
async fn the_finished_tournament_has_four_locked_rosters_inside_their_grants() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select e.status, e.locked_at, e.credit_grant, t.credit_grant as grant_of_tournament,
                t.roster_size, count(es.hero_id) as slots
         from tournament_entries e
             join tournaments t on t.id = e.tournament_id
             left join entry_slots es on es.entry_id = e.id
         where t.name = 'Summer of Legends'
         group by e.id, e.status, e.locked_at, e.credit_grant, t.credit_grant, t.roster_size",
    )
    .fetch_all(app.pool())
    .await
    .expect("entries");

    assert_eq!(4, rows.len());
    for row in &rows {
        assert_eq!("LOCKED", row.get::<String, _>("status"));
        assert!(
            row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("locked_at")
                .is_some()
        );
        assert_eq!(
            row.get::<i32, _>("roster_size") as i64,
            row.get::<i64, _>("slots")
        );
        assert_eq!(
            row.get::<i32, _>("grant_of_tournament"),
            row.get::<i32, _>("credit_grant")
        );
    }
}

#[tokio::test]
async fn every_seeded_roster_is_affordable_at_this_tournaments_prices() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select mg.handle, sum(th.cost)::int as spent, e.credit_grant
         from tournament_entries e
             join managers mg on mg.id = e.manager_id
             join entry_slots es on es.entry_id = e.id
             join tournament_heroes th
                 on th.tournament_id = e.tournament_id and th.hero_id = es.hero_id
         group by mg.handle, e.credit_grant
         order by mg.handle",
    )
    .fetch_all(app.pool())
    .await
    .expect("spends");

    let spends: Vec<(String, i32, i32)> = rows
        .iter()
        .map(|r| (r.get("handle"), r.get("spent"), r.get("credit_grant")))
        .collect();
    assert_eq!(
        vec![
            ("ArthurianLegend".to_owned(), 9_800, 10_000),
            ("MythicMind".to_owned(), 9_600, 10_000),
            ("NeonStrategist".to_owned(), 9_400, 10_000),
            ("SherlockMain".to_owned(), 9_600, 10_000),
        ],
        spends
    );
}

#[tokio::test]
async fn the_tournament_the_walkthrough_registers_for_is_left_empty() {
    let app = TestApp::spawn().await;
    let entries = scalar(
        app.pool(),
        "select count(*) from tournament_entries e
             join tournaments t on t.id = e.tournament_id
         where t.name = 'Winter of Champions'"
            .to_owned(),
    )
    .await;
    assert_eq!(0, entries);

    // `acceptsRegistration` is derived, not stored: an entry may be created
    // only while the tournament is open for it.
    let status: String =
        sqlx::query_scalar("select status from tournaments where name = 'Winter of Champions'")
            .fetch_one(app.pool())
            .await
            .expect("winter");
    assert_eq!("REGISTRATION_OPEN", status);
}

#[tokio::test]
async fn roster_slots_keep_their_draft_order_and_never_repeat_a_hero() {
    let app = TestApp::spawn().await;
    let repeats = scalar(
        app.pool(),
        "select count(*) from (
             select es.entry_id
             from entry_slots es
             group by es.entry_id
             having count(*) <> count(distinct es.hero_id)
         ) repeated"
            .to_owned(),
    )
    .await;
    assert_eq!(0, repeats);

    // `slot_index` is a list position, so it has to be a dense 0..n-1 run: a
    // gap would silently reorder a locked roster.
    let gaps = scalar(
        app.pool(),
        "select count(*) from (
             select entry_id
             from entry_slots
             group by entry_id
             having min(slot_index) <> 0
                 or max(slot_index)::bigint <> count(*) - 1
                 or count(distinct slot_index) <> count(*)
         ) gapped"
            .to_owned(),
    )
    .await;
    assert_eq!(
        0, gaps,
        "slot_index is the list position, 0..n-1 with no gaps"
    );
}

#[tokio::test]
async fn exactly_one_scoring_rule_set_is_active_per_tournament() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select t.name, count(rs.id) filter (where rs.is_active) as active_sets
         from tournaments t
             left join scoring_rule_sets rs on rs.tournament_id = t.id
         group by t.name
         order by t.name",
    )
    .fetch_all(app.pool())
    .await
    .expect("rule sets");

    let active: Vec<(String, i64)> = rows
        .iter()
        .map(|r| (r.get("name"), r.get("active_sets")))
        .collect();
    assert_eq!(
        vec![
            ("Spring of Myths".to_owned(), 1),
            ("Summer of Legends".to_owned(), 1),
            ("Winter of Champions".to_owned(), 1),
        ],
        active
    );
}

#[tokio::test]
async fn every_recorded_draft_is_complete_no_side_fielded_a_hero_it_never_drafted() {
    let app = TestApp::spawn().await;
    let undrafted = scalar(
        app.pool(),
        "select count(*)
         from match_game_participants mgp
             join match_games mg on mg.id = mgp.game_id
             left join match_hero_picks hp
                 on hp.match_id = mg.match_id and hp.side = mgp.side and hp.hero_id = mgp.hero_id
         where hp.hero_id is null"
            .to_owned(),
    )
    .await;

    assert_eq!(
        0, undrafted,
        "the same invariant MatchResultPolicy.PLAYED_HERO_NOT_DRAFTED enforces on the way in -- \
         a hero on the table but off the draft board would score no APPEARANCE at all"
    );
}

#[tokio::test]
async fn every_seeded_ban_is_sided_exactly_when_its_category_allows_one() {
    let app = TestApp::spawn().await;
    let db = app.pool();
    let misfiled = count(db, "hero_bans", "(ban_type = 'PRE_BAN') <> (side is null)").await;
    assert_eq!(
        0, misfiled,
        "a PRE_BAN precedes side assignment and carries no side, and V8 gave every other \
         seeded ban the side whose draft it came out of"
    );

    assert_eq!(
        13,
        count(db, "hero_bans", "side is null").await,
        "one pre-ban per seeded match"
    );
    assert_eq!(
        9,
        count(db, "hero_bans", "side is not null").await,
        "the 8 opponent bans + the Bo3's self ban"
    );
}

#[tokio::test]
async fn no_hero_is_both_drafted_and_banned_in_the_same_match() {
    let app = TestApp::spawn().await;
    let contradictions = scalar(
        app.pool(),
        "select count(*)
         from match_hero_picks hp
             join hero_bans hb on hb.match_id = hp.match_id and hb.hero_id = hp.hero_id"
            .to_owned(),
    )
    .await;
    assert_eq!(
        0, contradictions,
        "a hero struck out of the draft cannot then be taken in it"
    );
}

#[tokio::test]
async fn every_recorded_hero_was_in_the_tournaments_own_pool() {
    let app = TestApp::spawn().await;
    let strays = scalar(
        app.pool(),
        "select count(*)
         from match_game_participants mgp
             join match_games mg on mg.id = mgp.game_id
             left join tournament_heroes th
                 on th.tournament_id = mg.tournament_id and th.hero_id = mgp.hero_id
         where th.hero_id is null"
            .to_owned(),
    )
    .await;

    assert_eq!(
        0, strays,
        "a result naming a played hero outside the pool would poison every roster's score -- note \
         this checks who *played*, not who was *banned*: match 13's bans deliberately use \
         heroes outside Summer of Legends' pool, which this query never looks at"
    );
}

#[tokio::test]
async fn every_recorded_board_was_in_the_tournaments_own_map_pool() {
    let app = TestApp::spawn().await;
    let strays = scalar(
        app.pool(),
        "select count(*)
         from match_games mg
             left join tournament_maps tm
                 on tm.tournament_id = mg.tournament_id and tm.map_id = mg.map_id
         where tm.map_id is null"
            .to_owned(),
    )
    .await;
    assert_eq!(0, strays);
}

#[tokio::test]
async fn exactly_one_side_wins_every_seeded_game() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query(
        "select mg.id, count(*) filter (where mgp.is_winner) as winners
         from match_games mg
             join match_game_participants mgp on mgp.game_id = mg.id
         group by mg.id
         order by mg.id",
    )
    .fetch_all(app.pool())
    .await
    .expect("winners per game");

    let winners_per_game: Vec<(i64, i64)> = rows
        .iter()
        .map(|r| (r.get("id"), r.get("winners")))
        .collect();

    // Two winners is stopped by the partial unique index; zero is stopped only
    // by MatchResultPolicy, which the seed SQL bypasses -- so the seed's own
    // conformance to "every game is played to a decision" is asserted here.
    assert!(
        winners_per_game.iter().all(|&(_, w)| w <= 1),
        "the partial unique index should make this impossible"
    );
    let wrong: Vec<i64> = winners_per_game
        .iter()
        .filter(|&&(_, w)| w != 1)
        .map(|&(id, _)| id)
        .collect();
    assert_eq!(
        Vec::<i64>::new(),
        wrong,
        "every game has exactly one winner"
    );
}

#[tokio::test]
async fn the_database_rejects_positive_health_for_a_losing_hero() {
    let app = TestApp::spawn().await;
    let result = sqlx::query(
        "update match_game_participants
         set health_remaining = 1
         where game_id = 1 and side = 1",
    )
    .execute(app.pool())
    .await;

    assert!(
        matches!(result, Err(sqlx::Error::Database(ref e)) if e.code().is_some()),
        "expected a constraint violation, got {result:?}"
    );
}

#[tokio::test]
async fn the_database_rejects_a_game_recorded_without_a_map() {
    let app = TestApp::spawn().await;
    let result = sqlx::query(
        "insert into match_games (match_id, tournament_id, game_number, map_id)
         values (1, 1, 99, null)",
    )
    .execute(app.pool())
    .await;

    assert!(
        matches!(result, Err(sqlx::Error::Database(ref e)) if e.code().is_some()),
        "expected a constraint violation, got {result:?}"
    );
}

#[tokio::test]
async fn match_ids_ascend_with_played_at_which_is_what_makes_the_id_a_safe_polling_key() {
    let app = TestApp::spawn().await;
    let rows = sqlx::query("select id, played_at from tournament_matches order by id")
        .fetch_all(app.pool())
        .await
        .expect("matches");

    let played: Vec<(i64, chrono::DateTime<chrono::Utc>)> = rows
        .iter()
        .map(|r| (r.get("id"), r.get("played_at")))
        .collect();

    let mut by_time = played.clone();
    by_time.sort_by_key(|&(_, at)| at);
    assert_eq!(
        by_time.iter().map(|&(id, _)| id).collect::<Vec<_>>(),
        played.iter().map(|&(id, _)| id).collect::<Vec<_>>()
    );

    let mut distinct: Vec<_> = played.iter().map(|&(_, at)| at).collect();
    distinct.sort_by_key(|at| *at);
    distinct.dedup();
    assert!(
        distinct.len() < played.len(),
        "played_at must NOT be unique -- parallel tables share a start time"
    );
}

/// The one assertion here that is about the *runner* rather than the seed.
///
/// Flyway orders by version **across** locations. This pins the interleaving
/// that produces. Migrations are periodically squashed back to a `V1`
/// baseline (see `AGENTS.md`), so this currently has nothing to interleave --
/// `db/migration` is just `V1__core_schema.sql` and `V2__reference_data.sql`,
/// and `db/seed` is a single `V3__demo_fixtures.sql` sitting after both. The
/// assertion still earns its place: a *future* seed addition that depends on
/// a migration added after the current baseline (the way `V6__demo_draft_picks.sql`
/// and `V8__demo_ban_sides.sql` once did) gets its own version rather than an
/// edit to `V3`, and this is what would catch that version landing on the
/// wrong side of the schema it depends on.
#[test]
fn the_two_flyway_locations_interleave_by_version() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join("db/migration").is_dir())
        .expect("repository root")
        .join("db");
    let plan = crate::harness::migrate::plan(&root.join("migration"), Some(&root.join("seed")));

    let versions: Vec<u32> = plan.iter().map(|m| m.version).collect();
    assert_eq!(vec![1, 2, 3], versions);

    let seeded: Vec<u32> = plan
        .iter()
        .filter(|m| m.path.parent().is_some_and(|p| p.ends_with("seed")))
        .map(|m| m.version)
        .collect();
    assert_eq!(vec![3], seeded, "the seed's version sits after the schema's");

    // The `prod` shape: same schema and reference data, no league data at all.
    let without_seed = crate::harness::migrate::plan(&root.join("migration"), None);
    assert_eq!(
        vec![1, 2],
        without_seed.iter().map(|m| m.version).collect::<Vec<_>>()
    );
}
