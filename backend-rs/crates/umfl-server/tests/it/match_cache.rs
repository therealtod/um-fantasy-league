//! The match cache against the queries it stands in front of, on a real
//! database.
//!
//! Oracle: `match/MatchResultCacheIntegrationTest.kt`.
//!
//! The unit tests inside `match/cache.rs` cover the caching mechanics with a
//! stubbed loader. What needs Postgres is the one claim those cannot check:
//! that `MatchResultCache::find_by_tournament_since` -- a reverse, a filter and
//! a take over a list held in memory -- returns exactly what
//! `query::find_by_tournament_since`'s `order by played_at desc, id desc`
//! returns. The seed's *Summer of Legends* is the right fixture for it because
//! it has parallel tables that share a `played_at`, which is precisely the tie
//! the two orderings could disagree about.

use serde_json::{Value, json};
use umfl_domain::match_result::MatchResult;
use umfl_server::r#match::query;

use crate::harness::TestApp;

async fn summer(app: &TestApp) -> i64 {
    app.tournament_id("Summer of Legends").await
}

/// The cached slice must agree with the SQL for every boundary the ticker can
/// ask about -- ids on either side of the range, and limits both under and over
/// the tournament's match count, so truncation and exhaustion are both hit.
#[tokio::test]
async fn the_cached_ticker_slice_is_identical_to_the_query_it_replaces() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let mut conn = app.pool().acquire().await.unwrap();
    let all = query::find_by_tournament(&mut conn, summer, None)
        .await
        .unwrap();
    assert!(
        all.len() > 1,
        "the fixture needs several matches to be worth comparing"
    );

    for since_match_id in [0, 1, 6, 10, 13, 99] {
        for limit in [1, 3, 13, 25, 200] {
            let from_sql =
                query::find_by_tournament_since(&mut conn, summer, since_match_id, limit as i64)
                    .await
                    .unwrap();
            let from_cache = app
                .state
                .match_cache
                .find_by_tournament_since(app.pool(), summer, since_match_id, limit)
                .await
                .unwrap();
            assert_eq!(
                ids(&from_sql),
                ids(&from_cache),
                "slice disagreed with the SQL at sinceMatchId={since_match_id} limit={limit}"
            );
            assert_eq!(from_sql, from_cache);
        }
    }
}

fn ids(matches: &[MatchResult]) -> Vec<i64> {
    matches.iter().map(|m| m.match_id).collect()
}

#[tokio::test]
async fn a_cached_list_is_reused_rather_than_re_queried() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;

    let first = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    let second = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();

    assert!(!first.is_empty(), "the seed should have recorded matches");
    assert_eq!(first, second);
    // The same allocation, not merely an equal one -- proof the second read was
    // served rather than reloaded.
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(app.state.match_cache.cached_tournament_count().await, 1);
}

/// The invalidation pair, end to end: a write through the admin endpoint has to
/// be visible to the very next cache read, with nothing in between telling the
/// cache to look again.
#[tokio::test]
async fn a_match_recorded_through_the_api_reaches_the_very_next_read() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let before = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();

    record_a_match(&app, summer).await;

    let after = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert_eq!(
        after.len(),
        before.len() + 1,
        "the cache served a list from before the write"
    );
    assert!(
        after
            .iter()
            .any(|m| m.external_link == "urn:umfl:match:cache-test"),
        "the newly recorded match never appeared"
    );
}

/// A correction is a write too, and reuses an existing match id -- so the cache
/// has to drop the list rather than notice a new row was added.
#[tokio::test]
async fn correcting_a_match_reaches_the_very_next_read() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let recorded = record_a_match(&app, summer).await;
    let match_id = recorded["matchId"].as_i64().unwrap();
    let cached = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert!(cached.iter().any(|m| m.match_id == match_id));

    let mut corrected = recorded.clone();
    corrected["round"] = json!(9);
    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{summer}/matches/{match_id}"),
            app.manager("NeonStrategist").await.id,
            Some(&to_request(&corrected)),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let after = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    let round = after
        .iter()
        .find(|m| m.match_id == match_id)
        .expect("the corrected match")
        .round;
    assert_eq!(round, 9, "the cache served the pre-correction round");
}

/// A retraction, the third write -- the row is gone, so a stale list would
/// still be scoring points for a match that no longer exists.
#[tokio::test]
async fn deleting_a_match_reaches_the_very_next_read() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let match_id = record_a_match(&app, summer).await["matchId"]
        .as_i64()
        .unwrap();
    let before = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{summer}/matches/{match_id}"),
            app.manager("NeonStrategist").await.id,
            None,
        )
        .await;
    assert_eq!(response.status, 204, "{}", response.text());

    let after = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert_eq!(after.len(), before.len() - 1);
    assert!(after.iter().all(|m| m.match_id != match_id));
}

/// Renaming a **board** through the admin API drops every cached list, because
/// `mapName` is copied into an assembled match and no match write announces the
/// change. This is `AdminMapService.update`'s `ReferenceDataRenamedEvent`, end
/// to end.
#[tokio::test]
async fn renaming_a_board_reaches_the_very_next_read() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let cached = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    let map_id = cached
        .iter()
        .flat_map(|m| m.games.iter())
        .map(|g| g.map_id)
        .next()
        .expect("the seed's matches name boards");

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/maps/{map_id}"),
            app.manager("NeonStrategist").await.id,
            Some(&json!({ "name": "Renamed Board" })),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let after = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert!(
        after
            .iter()
            .flat_map(|m| m.games.iter())
            .any(|g| g.map_name == "Renamed Board"),
        "the cache served a list still spelling the old board name"
    );
}

/// The same staleness for a **hero**, whose admin service is not ported yet
/// (PORTING.md §3b) -- so the rename is made here the way that service will,
/// and this is what `AdminHeroService.update` has to keep passing once it
/// lands: without the global invalidation the old name survives.
#[tokio::test]
async fn a_hero_rename_needs_the_global_invalidation_to_be_seen() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;
    let cached = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    let hero_id = cached
        .iter()
        .flat_map(|m| m.games.iter())
        .flat_map(|g| g.participants.iter())
        .map(|p| p.hero_id)
        .next()
        .expect("the seed fields heroes");

    sqlx::query!(
        "update heroes set name = 'Renamed Hero' where id = $1",
        hero_id
    )
    .execute(app.pool())
    .await
    .unwrap();

    let stale = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert!(
        !names(&stale).contains(&"Renamed Hero".to_owned()),
        "without an invalidation the cached copy still spells the old name -- \
         which is the whole reason a rename has to announce itself"
    );

    app.state.match_cache.invalidate_all();

    let fresh = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .unwrap();
    assert!(names(&fresh).contains(&"Renamed Hero".to_owned()));
}

fn names(matches: &[MatchResult]) -> Vec<String> {
    matches
        .iter()
        .flat_map(|m| m.games.iter())
        .flat_map(|g| g.participants.iter())
        .map(|p| p.hero_name.clone())
        .collect()
}

/// Replays whatever the tournament's newest match already looks like rather
/// than hand-building a legal one -- `MatchResultPolicy` has opinions about
/// winners, health and drafts, and the subject here is the cache, not the
/// rules.
async fn record_a_match(app: &TestApp, tournament_id: i64) -> Value {
    let mut conn = app.pool().acquire().await.unwrap();
    let template = query::find_by_tournament(&mut conn, tournament_id, None)
        .await
        .unwrap()
        .pop()
        .expect("the seed has matches");
    drop(conn);

    // Both participant lists are ordered by `side`, and on the way in the side
    // *is* the list position -- so the query's own ordering is what makes this
    // round trip.
    let request = json!({
        "round": template.round,
        "playedAt": "2026-09-01T18:00:00Z",
        "externalLink": "urn:umfl:match:cache-test",
        "participants": template.participants.iter().map(|p| json!({
            "playerLabel": p.player_label,
            "draftedHeroIds": p.drafted_heroes.iter().map(|h| h.hero_id).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "games": template.games.iter().map(|g| json!({
            "gameNumber": g.game_number,
            "mapId": g.map_id,
            "participants": g.participants.iter().map(|p| json!({
                "heroId": p.hero_id,
                "healthRemaining": p.health_remaining,
                "isWinner": p.is_winner,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "bans": [],
    });

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{tournament_id}/matches"),
            app.manager("NeonStrategist").await.id,
            Some(&request),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()
}

/// A recorded match, back in the shape the record/correct endpoint accepts.
fn to_request(recorded: &Value) -> Value {
    json!({
        "round": recorded["round"],
        "playedAt": recorded["playedAt"],
        "externalLink": recorded["externalLink"],
        "participants": recorded["participants"].as_array().unwrap().iter().map(|p| json!({
            "playerLabel": p["playerLabel"],
            "draftedHeroIds": p["draftedHeroes"].as_array().unwrap()
                .iter().map(|h| h["heroId"].clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "games": recorded["games"].as_array().unwrap().iter().map(|g| json!({
            "gameNumber": g["gameNumber"],
            "mapId": g["mapId"],
            "participants": g["participants"].as_array().unwrap().iter().map(|p| json!({
                "heroId": p["heroId"],
                "healthRemaining": p["healthRemaining"],
                "isWinner": p["isWinner"],
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "bans": recorded["bans"].as_array().unwrap().iter().map(|b| json!({
            "heroId": b["heroId"],
            "banType": b["banType"],
            "side": b["side"],
        })).collect::<Vec<_>>(),
    })
}

/// The snapshot the cache loader and the standings service both open really is
/// REPEATABLE READ and really is read-only.
///
/// This is the one assertion that cannot be made by inspection, because getting
/// it wrong is not an error. `set transaction isolation level` is accepted only
/// before a transaction's first query; issued later it is silently ignored and
/// the transaction stays at READ COMMITTED (PORTING.md §7). Every statement
/// still succeeds and every row still looks plausible -- they just come from
/// several different snapshots -- so a regression here surfaces as a
/// leaderboard nobody can quite reproduce, not as a failure.
///
/// Asserted against `state::read_snapshot` itself, which is what
/// `standings::service::snapshot` and `match::cache`'s loader both now call.
#[tokio::test]
async fn the_read_snapshot_is_repeatable_read_and_read_only() {
    let app = TestApp::spawn().await;
    let mut tx = umfl_server::state::read_snapshot(app.pool())
        .await
        .expect("open the read snapshot");

    let isolation: String = sqlx::query_scalar("select current_setting('transaction_isolation')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        isolation, "repeatable read",
        "the isolation level was silently left at the default"
    );

    let read_only: String = sqlx::query_scalar("select current_setting('transaction_read_only')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(read_only, "on", "the snapshot must also be read-only");
}

/// A cache miss assembles its six queries inside that snapshot, so the list it
/// stores is internally coherent even if a write commits while it is loading.
///
/// The observable proxy is that the load still succeeds and still agrees with
/// the SQL: a loader that failed to open its transaction, or opened one it
/// could not read through, would show up here rather than in production.
#[tokio::test]
async fn a_cache_miss_assembles_its_queries_inside_one_snapshot() {
    let app = TestApp::spawn().await;
    let summer = summer(&app).await;

    let cached = app
        .state
        .match_cache
        .find_by_tournament(app.pool(), summer)
        .await
        .expect("a miss loads through its own snapshot");

    let mut conn = app.pool().acquire().await.unwrap();
    let direct = query::find_by_tournament(&mut conn, summer, None)
        .await
        .unwrap();

    assert!(!cached.is_empty(), "Summer of Legends has recorded matches");
    assert_eq!(cached.as_ref(), &direct);
}
