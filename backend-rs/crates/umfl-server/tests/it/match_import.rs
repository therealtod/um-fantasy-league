//! The admin match importer, with only the network boundary stubbed.
//!
//! Oracle: `matchimport/MatchImportServiceTest.kt` and
//! `matchimport/MatchImportEndpointTest.kt`, both driven over HTTP here.
//!
//! `fixtures/sample-match.json` is a genuine capture of a live scrape, copied
//! from the Kotlin suite's `resources/matchimport/sample-match.json` -- not a
//! hand-written sample. It is the actual shape the sidecar returns, including a
//! negative health value and a ten-hero draft. If the source site's markup
//! drifts, re-capturing it is what surfaces the change.
//!
//! Everything writes to **Summer of Legends**, whose seeded board pool
//! deliberately carries only one of this match's three boards -- which is what
//! makes `MAP_NOT_IN_POOL` observable without arranging anything.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use umfl_domain::DomainError;

use umfl_server::error::ApiResult;
use umfl_server::matchimport::ScraperClient;
use umfl_server::matchimport::scraper::ScrapedMatch;

use crate::harness::{TestApp, TestResponse};

const SOURCE_URL: &str = "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e";

/// The whole scraper, replaced by one canned answer.
///
/// `MatchImportServiceTest` uses a `@MockitoBean` for the same reason: the
/// alternative is a headless Chromium and a live source site inside the test
/// suite, and neither is what these tests are about.
struct StubScraper {
    outcome: Result<ScrapedMatch, DomainError>,
}

impl ScraperClient for StubScraper {
    fn scrape_match<'a>(&'a self, _source_url: &'a str) -> BoxFuture<'a, ApiResult<ScrapedMatch>> {
        Box::pin(async move { self.outcome.clone().map_err(Into::into) })
    }
}

fn sample_match() -> ScrapedMatch {
    serde_json::from_str(include_str!("fixtures/sample-match.json"))
        .expect("the captured fixture still deserializes")
}

/// An app whose scraper always returns this capture.
async fn app_with(scraped: ScrapedMatch) -> TestApp {
    TestApp::spawn_with(move |state| {
        state.scraper = Arc::new(StubScraper {
            outcome: Ok(scraped),
        });
    })
    .await
}

async fn app_with_scrape_error(error: DomainError) -> TestApp {
    TestApp::spawn_with(move |state| {
        state.scraper = Arc::new(StubScraper {
            outcome: Err(error),
        });
    })
    .await
}

async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

async fn summer(app: &TestApp) -> i64 {
    app.tournament_id("Summer of Legends").await
}

/// Every board in the fixture, so the preview comes back fully resolved.
///
/// Written straight to the link table: `tournament_maps` is a composite-keyed
/// table with no aggregate, and the point here is the importer, not the pool
/// endpoint that `map_admin.rs` already covers.
async fn stock_pool(app: &TestApp, tournament_id: i64) {
    for name in ["Technodrome", "Raptor Paddock", "Navy Pier"] {
        sqlx::query!(
            "insert into tournament_maps (tournament_id, map_id)
             select $1, id from game_maps where name = $2
             on conflict do nothing",
            tournament_id,
            name
        )
        .execute(app.pool())
        .await
        .unwrap_or_else(|e| panic!("add {name} to the pool: {e}"));
    }
}

async fn import(app: &TestApp, tournament_id: i64, source_url: &str) -> TestResponse {
    let admin = admin(app).await;
    app.send_as(
        "POST",
        &format!("/api/admin/tournaments/{tournament_id}/matches/import"),
        admin,
        Some(&json!({ "sourceUrl": source_url })),
    )
    .await
}

async fn preview(app: &TestApp, tournament_id: i64) -> Value {
    let response = import(app, tournament_id, SOURCE_URL).await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    response.json()
}

fn names(value: &Value, field: &str) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v[field].as_str().unwrap_or_default().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// The fixture itself
// ---------------------------------------------------------------------------

/// Sanity check on the capture, so a bad one fails loudly rather than skewing
/// every assertion below it.
#[test]
fn the_captured_fixture_deserializes_into_the_fields_the_importer_reads() {
    let scraped = sample_match();

    assert_eq!(scraped.round_name.as_deref(), Some("The Wayward Sisters"));
    assert_eq!(scraped.series_format.as_deref(), Some("BO3"));
    assert_eq!(
        scraped.played_at_raw.as_deref(),
        Some("17 Aug 2026, 22:00 CEST")
    );
    assert_eq!(
        scraped
            .side_a
            .as_ref()
            .and_then(|s| s.player_label.as_deref()),
        Some("mystic_owl")
    );
    assert_eq!(scraped.games.len(), 3);
    assert_eq!(scraped.pre_bans.len(), 6);
    assert_eq!(
        scraped.games[2].side_a.as_ref().and_then(|s| s.health),
        Some(-2),
        "an overkill finish is negative, and the policy exists to allow it"
    );
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolves_every_hero_and_board_when_the_pool_has_them_all() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    stock_pool(&app, summer).await;

    let preview = preview(&app, summer).await;

    assert!(
        preview["unresolved"]
            .as_array()
            .expect("unresolved")
            .is_empty(),
        "{preview:#}"
    );
    let games = preview["games"].as_array().expect("games");
    assert_eq!(games.len(), 3);
    assert!(games.iter().all(|g| g["mapId"].is_i64()));
    assert!(
        games
            .iter()
            .flat_map(|g| g["participants"].as_array().expect("participants"))
            .all(|p| p["heroId"].is_i64())
    );
    assert!(
        preview["bans"]
            .as_array()
            .expect("bans")
            .iter()
            .all(|b| b["heroId"].is_i64())
    );
    assert!(
        preview["participants"]
            .as_array()
            .expect("participants")
            .iter()
            .all(|p| p["draftedHeroIds"].as_array().expect("ids").len() == 3)
    );
}

#[tokio::test]
async fn maps_the_series_faithfully() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    stock_pool(&app, summer).await;

    let preview = preview(&app, summer).await;

    assert_eq!(
        names(&preview["participants"], "playerLabel"),
        ["mystic_owl", "immortal"]
    );
    let games = preview["games"].as_array().expect("games");
    assert_eq!(
        games
            .iter()
            .map(|g| g["gameNumber"].as_i64().expect("a game number"))
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        names(&preview["games"], "mapName"),
        ["Technodrome", "Raptor Paddock", "Navy Pier"]
    );

    // Game 1: side A wins on 5, side B is defeated on exactly 0.
    let game1 = games[0]["participants"].as_array().expect("participants");
    assert_eq!(
        game1
            .iter()
            .map(|p| p["isWinner"] == true)
            .collect::<Vec<_>>(),
        [true, false]
    );
    assert_eq!(
        game1
            .iter()
            .map(|p| p["healthRemaining"].as_i64().expect("health"))
            .collect::<Vec<_>>(),
        [5, 0]
    );

    // Game 3: the loser finished below zero on an overkill hit -- the value
    // `LOSER_HAS_POSITIVE_HEALTH` exists to allow.
    let game3 = games[2]["participants"].as_array().expect("participants");
    assert_eq!(game3[0]["healthRemaining"].as_i64(), Some(-2));
    assert_eq!(
        game3
            .iter()
            .map(|p| p["isWinner"] == true)
            .collect::<Vec<_>>(),
        [false, true]
    );

    assert_eq!(preview["roundName"], "The Wayward Sisters");
    assert_eq!(preview["seriesFormat"], "BO3");
    // 22:00 CEST is 20:00Z.
    assert_eq!(preview["playedAt"], "2026-08-17T20:00:00Z");
    assert!(
        preview.get("round").is_none(),
        "the source names its pools; only a human can supply a round number"
    );
}

/// Both sides' typed bans plus the shared pre-ban pool land in one flat list,
/// as `hero_bans` stores them.
#[tokio::test]
async fn flattens_both_sides_bans_and_the_pre_ban_pool() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;

    let preview = preview(&app, summer).await;

    let bans = preview["bans"].as_array().expect("bans").clone();
    assert_eq!(bans.len(), 10);
    let of_type = |kind: &str| bans.iter().filter(|b| b["banType"] == kind).count();
    assert_eq!(of_type("OPPONENT_BAN"), 2);
    assert_eq!(of_type("SELF_BAN"), 2);
    assert_eq!(of_type("PRE_BAN"), 6);

    let mut opponent_bans: Vec<String> = bans
        .iter()
        .filter(|b| b["banType"] == "OPPONENT_BAN")
        .map(|b| b["heroName"].as_str().expect("a hero").to_owned())
        .collect();
    opponent_bans.sort();
    assert_eq!(opponent_bans, ["Alice", "John Henry"]);

    // Picks and bans stay disjoint, or `BANNED_HERO_DRAFTED` would fire on save.
    let drafted: Vec<i64> = preview["participants"]
        .as_array()
        .expect("participants")
        .iter()
        .flat_map(|p| p["draftedHeroIds"].as_array().expect("ids"))
        .map(|v| v.as_i64().expect("an id"))
        .collect();
    assert!(
        bans.iter()
            .filter_map(|b| b["heroId"].as_i64())
            .all(|id| !drafted.contains(&id))
    );
}

/// Flat, but not side-blind: the source files a typed ban under the side that
/// owned the hero, and `hero_bans.side` keeps it. A pre-ban is struck before
/// sides are assigned and carries none, which is what `BAN_SIDE_INVALID`
/// insists on.
#[tokio::test]
async fn keeps_the_side_a_typed_ban_was_struck_from_and_leaves_a_pre_ban_unsided() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;

    let preview = preview(&app, summer).await;
    let bans = preview["bans"].as_array().expect("bans");

    assert!(
        bans.iter()
            .filter(|b| b["banType"] == "PRE_BAN")
            .all(|b| b.get("side").is_none()),
        "a pre-ban belongs to neither side, and `non_null` omits rather than emits"
    );
    let sided: Vec<(String, i64)> = bans
        .iter()
        .filter(|b| b["banType"] != "PRE_BAN")
        .map(|b| {
            (
                b["heroName"].as_str().expect("a hero").to_owned(),
                b["side"].as_i64().expect("a side"),
            )
        })
        .collect();
    assert_eq!(
        sided,
        [
            ("Alice".to_owned(), 0),
            ("Daredevil".to_owned(), 0),
            ("John Henry".to_owned(), 1),
            ("Dr. Jill Trent".to_owned(), 1),
        ],
        "side A's two bans came out of side A's draft, side B's out of side B's"
    );
}

/// The board exists in `game_maps` but not in this tournament's pool. It is
/// reported, not invented -- `match_games`'s composite FK onto `tournament_maps`
/// means recording a game on it would fail at the database.
#[tokio::test]
async fn reports_a_board_that_is_missing_from_this_tournaments_pool() {
    // The seed's Summer pool has Raptor Paddock but neither Technodrome nor
    // Navy Pier, so this deliberately does *not* stock the pool.
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;

    let preview = preview(&app, summer).await;

    let unresolved = preview["unresolved"].as_array().expect("unresolved");
    let missing: Vec<&Value> = unresolved
        .iter()
        .filter(|u| u["reason"] == "MAP_NOT_IN_POOL")
        .collect();
    let mut missing_names: Vec<&str> = missing
        .iter()
        .map(|u| u["sourceName"].as_str().expect("a name"))
        .collect();
    missing_names.sort();
    assert_eq!(missing_names, ["Navy Pier", "Technodrome"]);
    assert!(missing.iter().all(|u| u["kind"] == "MAP"));
    // The id is carried so the client can offer to add it to the pool in one
    // click -- `MatchImportPanel`'s "Add to board pool" button.
    assert!(missing.iter().all(|u| u["mapId"].is_i64()));

    let games = preview["games"].as_array().expect("games");
    assert!(
        games[0].get("mapId").is_none(),
        "the unresolved game has no map"
    );
    assert!(games[1]["mapId"].is_i64(), "the resolvable one still does");
    // Heroes are unaffected: they reference `heroes(id)`, never
    // `tournament_heroes`.
    assert!(unresolved.iter().all(|u| u["kind"] != "HERO"));
}

/// A hero the catalogue has never heard of is named once, not once per
/// appearance.
#[tokio::test]
async fn reports_an_unknown_hero_exactly_once() {
    const GHOST: &str = "Nonexistent Hero";
    let mut scraped = sample_match();
    let side_a = scraped.side_a.as_mut().expect("the fixture has both sides");
    side_a.picks[0] = GHOST.to_owned();
    scraped.games[0]
        .side_a
        .as_mut()
        .expect("game 1 has both sides")
        .hero_name = Some(GHOST.to_owned());

    let app = app_with(scraped).await;
    let summer = summer(&app).await;
    let preview = preview(&app, summer).await;

    let unknown: Vec<&Value> = preview["unresolved"]
        .as_array()
        .expect("unresolved")
        .iter()
        .filter(|u| u["reason"] == "UNKNOWN_HERO")
        .collect();
    assert_eq!(unknown.len(), 1, "named once, though it appears twice");
    assert_eq!(unknown[0]["sourceName"], GHOST);

    let first_side = &preview["games"][0]["participants"][0];
    assert!(first_side.get("heroId").is_none());
    // The name is still carried so the admin can see what failed.
    assert_eq!(first_side["heroName"], GHOST);
}

/// A timezone the parser cannot resolve costs the timestamp, never the import.
#[tokio::test]
async fn leaves_played_at_absent_when_the_timestamp_is_unparseable() {
    let mut scraped = sample_match();
    scraped.played_at_raw = Some("sometime on Tuesday".to_owned());
    let app = app_with(scraped).await;
    let summer = summer(&app).await;

    let preview = preview(&app, summer).await;

    assert!(preview.get("playedAt").is_none(), "{preview:#}");
    assert_eq!(preview["playedAtRaw"], "sometime on Tuesday");
    assert_eq!(preview["games"].as_array().expect("games").len(), 3);
}

#[tokio::test]
async fn reports_no_duplicate_when_this_url_has_not_been_imported() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;

    let preview = preview(&app, summer).await;

    assert!(preview.get("alreadyImportedMatchId").is_none());
}

// ---------------------------------------------------------------------------
// The round trip that matters
// ---------------------------------------------------------------------------

/// Builds exactly what the client builds: the preview, plus the round number
/// only a human can supply.
fn record_body(preview: &Value, round: i32) -> Value {
    json!({
        "round": round,
        "playedAt": preview["playedAt"],
        "externalLink": preview["sourceUrl"],
        "participants": preview["participants"],
        "games": preview["games"],
        "bans": preview["bans"],
    })
}

/// The importer knows nothing about the match rules -- it resolves names and
/// stops. That is only safe if the draft it produces is actually recordable,
/// and the two are far enough apart that nothing else would catch a drift.
/// **This test is that seam**: it fails if the importer ever starts emitting
/// something the policy rejects (a hero played but not drafted, a ban colliding
/// with a pick, non-sequential game numbers, a loser left on positive health).
#[tokio::test]
async fn a_fully_resolved_preview_is_accepted_verbatim_by_the_record_endpoint() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    stock_pool(&app, summer).await;
    let admin = admin(&app).await;

    let preview = preview(&app, summer).await;
    assert!(
        preview["unresolved"]
            .as_array()
            .expect("unresolved")
            .is_empty(),
        "expected a fully resolved preview: {preview:#}"
    );

    let recorded = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{summer}/matches"),
            admin,
            Some(&record_body(&preview, 1)),
        )
        .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());

    let body = recorded.json();
    assert_eq!(body["round"], 1);
    assert_eq!(body["games"].as_array().expect("games").len(), 3);
    assert_eq!(body["bans"].as_array().expect("bans").len(), 10);
    assert_eq!(body["externalLink"], SOURCE_URL);

    // The side survives the whole round trip: the source grouped these under
    // the side that owned the hero, the preview kept it, and `hero_bans.side`
    // stored it.
    let bans = body["bans"].as_array().expect("bans");
    assert_eq!(
        [0, 1].map(|side| bans.iter().filter(|b| b["side"] == side).count()),
        [2, 2],
        "each side's two typed bans came back attributed to it"
    );
    assert!(
        bans.iter()
            .filter(|b| b["banType"] == "PRE_BAN")
            .all(|b| b.get("side").is_none()),
        "a pre-ban precedes side assignment, so the null side is omitted entirely"
    );
}

/// Re-importing a URL already recorded names the existing match instead of
/// silently duplicating it -- and recording a second copy is refused outright,
/// so the preview's warning and the write path agree rather than the admin
/// discovering the conflict on save.
#[tokio::test]
async fn a_second_import_of_the_same_url_reports_the_existing_match_and_cannot_be_recorded_twice() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    stock_pool(&app, summer).await;
    let admin = admin(&app).await;

    let first = preview(&app, summer).await;
    assert!(first.get("alreadyImportedMatchId").is_none());

    let body = record_body(&first, 1);
    let recorded = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{summer}/matches"),
            admin,
            Some(&body),
        )
        .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());
    let match_id = recorded.json()["matchId"].as_i64().expect("a match id");

    let second = preview(&app, summer).await;
    assert_eq!(second["alreadyImportedMatchId"].as_i64(), Some(match_id));

    // The link is what makes the duplicate detectable, so posting the same
    // draft again is a 409 naming the match to correct -- not a second row
    // quietly double-counting every point this match scores.
    let duplicate = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{summer}/matches"),
            admin,
            Some(&body),
        )
        .await;
    assert_eq!(duplicate.status, 409, "{}", duplicate.text());
    assert!(
        duplicate.json()["detail"]
            .as_str()
            .expect("a detail")
            .contains(&match_id.to_string()),
        "{}",
        duplicate.text()
    );
}

// ---------------------------------------------------------------------------
// The failure paths
// ---------------------------------------------------------------------------

/// A URL the client should not have sent is a clean 409, and never a scrape
/// attempt -- the stub here would answer happily if it were reached.
#[tokio::test]
async fn a_url_that_is_not_a_match_page_is_rejected_without_scraping() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;

    let response = import(&app, summer, "https://example.com/nope").await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        "Only www.tabletopleague.com match pages can be imported."
    );
}

/// A scraper that is not running is a 503 telling the admin how to start it --
/// not a 500, and not a silent empty preview.
#[tokio::test]
async fn a_scraper_that_is_down_is_a_503_naming_how_to_start_it() {
    let app = app_with_scrape_error(DomainError::service_unavailable(
        "The match scraper at http://localhost:3000 is not reachable. \
         Start it with `npm run serve` in tools/tabletopleague-scraper, \
         or bring up the `scraper` service. (Connection refused)",
    ))
    .await;
    let summer = summer(&app).await;

    let response = import(&app, summer, SOURCE_URL).await;

    assert_eq!(response.status, 503, "{}", response.text());
    assert!(
        response.json()["detail"]
            .as_str()
            .expect("a detail")
            .contains("npm run serve"),
        "{}",
        response.text()
    );
}

/// A scraper that answered but could not read the page is a 409: it *is*
/// running, so retrying will not help -- usually selector drift after the
/// source site changed its markup.
#[tokio::test]
async fn a_scraper_that_could_not_read_the_page_is_a_409() {
    let app = app_with_scrape_error(DomainError::conflict(
        "The scraper could not read that match page: no such match",
    ))
    .await;
    let summer = summer(&app).await;

    let response = import(&app, summer, SOURCE_URL).await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        "The scraper could not read that match page: no such match"
    );
}

/// A match the scraper only half-read is refused rather than previewed with one
/// side missing -- there is nothing for the admin to correct in a draft with no
/// opponent.
#[tokio::test]
async fn a_scrape_missing_a_side_is_refused() {
    let mut scraped = sample_match();
    scraped.side_b = None;
    let app = app_with(scraped).await;
    let summer = summer(&app).await;

    let response = import(&app, summer, SOURCE_URL).await;

    assert_eq!(response.status, 409, "{}", response.text());
    assert!(
        response.json()["detail"]
            .as_str()
            .expect("a detail")
            .starts_with("The scraper could not read both sides of that match."),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn an_unknown_tournament_is_a_404_before_anything_is_scraped() {
    let app = app_with(sample_match()).await;

    let response = import(&app, 999_999, SOURCE_URL).await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No tournament with id 999999");
}

#[tokio::test]
async fn a_blank_source_url_is_a_400_naming_the_field() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    let admin = admin(&app).await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{summer}/matches/import"),
            admin,
            Some(&json!({ "sourceUrl": "   " })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["sourceUrl"],
        "sourceUrl is required"
    );
}

#[tokio::test]
async fn importing_is_admin_only() {
    let app = app_with(sample_match()).await;
    let summer = summer(&app).await;
    let non_admin = app.manager("SherlockMain").await.id;
    let uri = format!("/api/admin/tournaments/{summer}/matches/import");
    let body = json!({ "sourceUrl": SOURCE_URL });

    let forbidden = app.send_as("POST", &uri, non_admin, Some(&body)).await;
    assert_eq!(forbidden.status, 403, "{}", forbidden.text());

    let anonymous = app.post_json(&uri, &body).await;
    assert_eq!(anonymous.status, 401, "{}", anonymous.text());
}
