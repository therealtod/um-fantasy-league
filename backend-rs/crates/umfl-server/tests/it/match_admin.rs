//! Recorded match results, through
//! `/api/admin/tournaments/{id}/matches`.
//!
//! Oracle: `match/AdminMatchServiceIntegrationTest.kt`, driven over HTTP rather
//! than against the service, so the DTO shape and the status codes are checked
//! by the same test that checks the rule.
//!
//! Everything here writes to **Winter of Champions**, which the seed
//! deliberately leaves with zero recorded matches -- *Summer of Legends* is
//! left alone because other tests assert its exact fixtures.

use serde_json::{Value, json};

use crate::harness::{TestApp, TestResponse};

/// *NeonStrategist* is the seed's only `is_admin` manager.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

/// A source link no other match holds. `external_link` is required and unique
/// within a tournament, so every recorded match needs one; the value itself is
/// beside the point for tests that assert on something else.
fn a_link() -> String {
    format!("https://example.com/match/{}", uuid::Uuid::new_v4())
}

async fn map_id(app: &TestApp, name: &str) -> i64 {
    sqlx::query_scalar!("select id from game_map where name = $1", name)
        .fetch_one(app.pool())
        .await
        .unwrap_or_else(|e| panic!("no board {name}: {e}"))
}

/// Both sides draft every hero these fixtures field. A recorded draft has to
/// cover whatever the games use (`PLAYED_HERO_NOT_DRAFTED`), and the games
/// below trade heroes across sides freely, so one shared draft keeps each test
/// about the rule it is actually testing. Bigfoot is deliberately not in it: it
/// is the hero these fixtures ban.
async fn fixture_draft(app: &TestApp) -> Vec<i64> {
    app.hero_ids(&["Alice", "Robin Hood", "Medusa", "Achilles", "King Arthur"])
        .await
}

fn participant(label: Option<&str>, drafted: &[i64]) -> Value {
    match label {
        Some(label) => json!({ "playerLabel": label, "draftedHeroIds": drafted }),
        None => json!({ "draftedHeroIds": drafted }),
    }
}

async fn two_sides(app: &TestApp) -> Value {
    let drafted = fixture_draft(app).await;
    json!([
        participant(Some("Tomas Ferreira"), &drafted),
        participant(Some("Rina Okafor"), &drafted),
    ])
}

/// One game: side 0's hero wins on `health_a`, side 1's loses on `health_b`.
fn game(
    game_number: i32,
    map: i64,
    (hero_a, health_a, wins_a): (i64, i32, bool),
    (hero_b, health_b, wins_b): (i64, i32, bool),
) -> Value {
    json!({
        "gameNumber": game_number,
        "mapId": map,
        "participants": [
            { "heroId": hero_a, "healthRemaining": health_a, "isWinner": wins_a },
            { "heroId": hero_b, "healthRemaining": health_b, "isWinner": wins_b },
        ],
    })
}

/// The default fixture game: Alice takes Baskerville Manor off Robin Hood.
async fn alice_beats_robin_hood(app: &TestApp, map: i64) -> Value {
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    json!([game(1, map, (alice, 6, true), (robin_hood, 0, false))])
}

fn body(round: i32, link: &str, participants: Value, games: Value, bans: Value) -> Value {
    json!({
        "round": round,
        "playedAt": "2026-03-01T18:00:00Z",
        "externalLink": link,
        "participants": participants,
        "games": games,
        "bans": bans,
    })
}

async fn post_match(app: &TestApp, tournament_id: i64, body: &Value) -> TestResponse {
    app.send_as(
        "POST",
        &format!("/api/admin/tournaments/{tournament_id}/matches"),
        admin(app).await,
        Some(body),
    )
    .await
}

async fn put_match(app: &TestApp, tournament_id: i64, match_id: i64, body: &Value) -> TestResponse {
    app.send_as(
        "PUT",
        &format!("/api/admin/tournaments/{tournament_id}/matches/{match_id}"),
        admin(app).await,
        Some(body),
    )
    .await
}

/// Records the fixture match and returns the created body.
async fn record_fixture(app: &TestApp, tournament_id: i64, round: i32) -> Value {
    let map = map_id(app, "Baskerville Manor").await;
    let request = body(
        round,
        &a_link(),
        two_sides(app).await,
        alice_beats_robin_hood(app, map).await,
        json!([]),
    );
    let response = post_match(app, tournament_id, &request).await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()
}

/// The rule codes off a 422, in the order the policy reported them.
fn rules(response: &TestResponse) -> Vec<String> {
    response.json()["violations"]
        .as_array()
        .expect("a violations array")
        .iter()
        .filter_map(|v| v["rule"].as_str().map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

#[tokio::test]
async fn records_a_match_result_against_a_tournament_with_none_yet() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let recorded = record_fixture(&app, winter, 1).await;

    assert_eq!(recorded["participants"].as_array().unwrap().len(), 2);
    let games = recorded["games"].as_array().unwrap();
    assert_eq!(games.len(), 1);
    let alice = games[0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["heroName"] == "Alice")
        .expect("Alice is in the game");
    assert_eq!(alice["isWinner"], true);

    let mut conn = app.pool().acquire().await.unwrap();
    let stored = umfl_server::r#match::query::find_by_tournament(&mut conn, winter, None)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
}

/// PORTING.md deviation (b): `GameResult.winner` is a computed Kotlin property
/// Jackson emits as an undeclared field, reproduced on the DTO so the port does
/// not quietly change the wire. Nothing in the frontend reads it.
#[tokio::test]
async fn a_game_carries_the_undeclared_winner_field_jackson_emitted() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let recorded = record_fixture(&app, winter, 1).await;

    let winner = &recorded["games"][0]["winner"];
    assert_eq!(winner["heroName"], "Alice");
    assert_eq!(winner["isWinner"], true);
}

#[tokio::test]
async fn find_by_tournament_narrows_to_one_round_when_asked() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    record_fixture(&app, winter, 1).await;
    record_fixture(&app, winter, 2).await;

    let mut conn = app.pool().acquire().await.unwrap();
    let all = umfl_server::r#match::query::find_by_tournament(&mut conn, winter, None)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    let round_one = umfl_server::r#match::query::find_by_tournament(&mut conn, winter, Some(1))
        .await
        .unwrap();
    assert_eq!(round_one.iter().map(|m| m.round).collect::<Vec<_>>(), [1]);
}

/// The admin list is the endpoint over `find_by_tournament_newest_first`: the
/// exact reverse of the fold's order, and bounded from the *newest* end.
#[tokio::test]
async fn the_admin_list_reverses_the_folds_order_and_honours_its_limit() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    for round in 1..=3 {
        let request = json!({
            "round": round,
            // Distinct timestamps, so `played_at desc` alone settles the order
            // and the `id desc` tiebreak is not what is under test here.
            "playedAt": format!("2026-03-0{round}T18:00:00Z"),
            "externalLink": a_link(),
            "participants": two_sides(&app).await,
            "games": alice_beats_robin_hood(&app, map).await,
            "bans": [],
        });
        assert_eq!(post_match(&app, winter, &request).await.status, 201);
    }
    let uri = format!("/api/admin/tournaments/{winter}/matches");

    let listed = app.get_as(&uri, admin(&app).await).await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    listed.assert_no_json_nulls();
    assert_eq!(rounds(&listed.json()), [3, 2, 1]);

    // The bound takes the newest page, not the oldest one.
    let bounded = app
        .get_as(&format!("{uri}?limit=2"), admin(&app).await)
        .await;
    assert_eq!(rounds(&bounded.json()), [3, 2]);

    let filtered = app
        .get_as(&format!("{uri}?round=2"), admin(&app).await)
        .await;
    assert_eq!(rounds(&filtered.json()), [2]);
}

fn rounds(body: &Value) -> Vec<i64> {
    body.as_array()
        .expect("an array")
        .iter()
        .filter_map(|m| m["round"].as_i64())
        .collect()
}

#[tokio::test]
async fn records_a_best_of_three_with_a_hero_repeated_across_games_and_different_maps() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let baskerville = map_id(&app, "Baskerville Manor").await;
    let sherwood = map_id(&app, "Sherwood Forest").await;
    let medusa = app.hero_id("Medusa").await;
    let achilles = app.hero_id("Achilles").await;

    let request = body(
        1,
        "https://example.com/bracket/7",
        two_sides(&app).await,
        json!([
            game(1, sherwood, (medusa, 6, true), (achilles, 0, false)),
            game(2, baskerville, (medusa, 0, false), (achilles, 5, true)),
            game(3, sherwood, (medusa, 3, true), (achilles, 0, false)),
        ]),
        json!([]),
    );
    let response = post_match(&app, winter, &request).await;
    assert_eq!(response.status, 201, "{}", response.text());

    let recorded = response.json();
    let games = recorded["games"].as_array().unwrap();
    assert_eq!(games.len(), 3);
    assert_eq!(
        games
            .iter()
            .map(|g| g["gameNumber"].as_i64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        games
            .iter()
            .map(|g| g["mapId"].as_i64())
            .collect::<Vec<_>>(),
        [Some(sherwood), Some(baskerville), Some(sherwood)]
    );
    // `WIN`/`LOSS` are scored per game, so a hero that takes game 1 and drops
    // game 2 collects one of each -- which is why the winner alternates here.
    assert_eq!(games[0]["winner"]["heroName"], "Medusa");
    assert_eq!(games[1]["winner"]["heroName"], "Achilles");
    assert_eq!(games[2]["winner"]["heroName"], "Medusa");
}

// ---------------------------------------------------------------------------
// The rules, as 422s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_map_outside_the_tournaments_board_pool_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    // Raptor Paddock is not in Winter of Champions' seeded board pool.
    let raptor_paddock = map_id(&app, "Raptor Paddock").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        alice_beats_robin_hood(&app, raptor_paddock).await,
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(
        response.json()["type"],
        "https://umfl.dev/problems/match-rule-violation"
    );
    assert_eq!(rules(&response), ["MAP_NOT_IN_POOL"]);
}

#[tokio::test]
async fn the_same_hero_on_both_sides_of_a_game_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        json!([game(1, map, (alice, 6, true), (alice, 0, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["DUPLICATE_HERO"]);
}

#[tokio::test]
async fn two_winners_on_one_game_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        json!([game(1, map, (alice, 6, true), (robin_hood, 0, true))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["NOT_EXACTLY_ONE_WINNER"]);
}

/// There is no draw. Zero winners is as wrong as two.
#[tokio::test]
async fn a_game_with_no_winner_at_all_is_rejected_not_stored_as_a_draw() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        json!([game(1, map, (alice, 0, false), (robin_hood, 0, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["NOT_EXACTLY_ONE_WINNER"]);
}

#[tokio::test]
async fn a_game_with_a_positive_health_loser_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        json!([game(1, map, (alice, 7, true), (robin_hood, 5, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["LOSER_HAS_POSITIVE_HEALTH"]);
}

/// The loser never survives, but an overkill hit lands it *below* zero -- which
/// is legal, and is what `HEALTH_DIFFERENTIAL` prices.
#[tokio::test]
async fn a_game_with_a_negative_health_loser_is_accepted() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let request = body(
        1,
        &a_link(),
        two_sides(&app).await,
        json!([game(1, map, (alice, 7, true), (robin_hood, -3, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 201, "{}", response.text());
    let recorded = response.json();
    let loser = recorded["games"][0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["isWinner"] == false)
        .expect("a loser");
    assert_eq!(loser["healthRemaining"], -3);
}

#[tokio::test]
async fn a_nonexistent_hero_id_is_a_422_not_a_raw_constraint_violation() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let mut drafted = fixture_draft(&app).await;
    drafted.push(999_999);
    let request = body(
        1,
        &a_link(),
        json!([
            participant(Some("Tomas Ferreira"), &drafted),
            participant(Some("Rina Okafor"), &drafted),
        ]),
        json!([game(1, map, (999_999, 6, true), (robin_hood, 0, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["UNKNOWN_HERO"]);
}

/// A hero cannot be struck out of the draft and then taken in it, so a banned
/// hero that played breaks both ban rules rather than one.
#[tokio::test]
async fn a_hero_banned_then_played_in_a_later_game_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let mut drafted = fixture_draft(&app).await;
    drafted.push(bigfoot);
    let request = body(
        1,
        &a_link(),
        json!([
            participant(Some("Tomas Ferreira"), &drafted),
            participant(Some("Rina Okafor"), &drafted),
        ]),
        json!([
            game(1, map, (alice, 6, true), (robin_hood, 0, false)),
            game(2, map, (bigfoot, 4, true), (robin_hood, 0, false)),
        ]),
        json!([{ "heroId": bigfoot, "banType": "PRE_BAN" }]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    let mut broken = rules(&response);
    broken.sort();
    assert_eq!(broken, ["BANNED_HERO_DRAFTED", "BANNED_HERO_PLAYED"]);
}

#[tokio::test]
async fn a_hero_fielded_by_a_side_that_never_drafted_it_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let request = body(
        1,
        &a_link(),
        json!([
            participant(Some("Tomas Ferreira"), &[alice]),
            participant(Some("Rina Okafor"), &[alice]),
        ]),
        json!([game(1, map, (alice, 6, true), (robin_hood, 0, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;

    assert_eq!(response.status, 422, "{}", response.text());
    assert_eq!(rules(&response), ["PLAYED_HERO_NOT_DRAFTED"]);
    let message = response.json()["violations"][0]["message"]
        .as_str()
        .expect("a message")
        .to_owned();
    assert!(
        message.contains(&format!("{robin_hood} in game 1")),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// The draft
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_sides_draft_round_trips_including_a_hero_it_never_fielded() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let medusa = app.hero_id("Medusa").await;
    let request = body(
        1,
        &a_link(),
        json!([
            participant(Some("Tomas Ferreira"), &[alice, medusa]),
            participant(Some("Rina Okafor"), &[robin_hood]),
        ]),
        json!([game(1, map, (alice, 6, true), (robin_hood, 0, false))]),
        json!([]),
    );

    let response = post_match(&app, winter, &request).await;
    assert_eq!(response.status, 201, "{}", response.text());

    let recorded = response.json();
    assert_eq!(drafted_names(&recorded, 0), ["Alice", "Medusa"]);
    assert_eq!(drafted_names(&recorded, 1), ["Robin Hood"]);
    let fielded: Vec<&str> = recorded["games"][0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["heroName"].as_str())
        .collect();
    assert!(
        !fielded.contains(&"Medusa"),
        "Medusa was drafted and never fielded -- exactly the case APPEARANCE prices"
    );
}

fn drafted_names(body: &Value, side: i64) -> Vec<String> {
    body["participants"]
        .as_array()
        .expect("an array")
        .iter()
        .find(|p| p["side"] == side)
        .expect("that side")["draftedHeroes"]
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|h| h["heroName"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn correcting_a_match_replaces_the_draft_rather_than_adding_to_it() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let medusa = app.hero_id("Medusa").await;
    let games = json!([game(1, map, (alice, 6, true), (robin_hood, 0, false))]);

    let recorded = post_match(
        &app,
        winter,
        &body(
            1,
            &a_link(),
            json!([
                participant(Some("Tomas Ferreira"), &[alice, medusa]),
                participant(Some("Rina Okafor"), &[robin_hood]),
            ]),
            games.clone(),
            json!([]),
        ),
    )
    .await
    .json();
    let match_id = recorded["matchId"].as_i64().expect("an id");

    let corrected = put_match(
        &app,
        winter,
        match_id,
        &body(
            1,
            &a_link(),
            json!([
                participant(Some("Tomas Ferreira"), &[alice]),
                participant(Some("Rina Okafor"), &[robin_hood]),
            ]),
            games,
            json!([]),
        ),
    )
    .await;
    assert_eq!(corrected.status, 200, "{}", corrected.text());

    assert_eq!(
        drafted_names(&corrected.json(), 0),
        ["Alice"],
        "the mistaken Medusa pick is gone, not merged"
    );
    let picks = sqlx::query_scalar!(
        r#"select count(*) as "count!" from match_hero_pick where match_id = $1"#,
        match_id
    )
    .fetch_one(app.pool())
    .await
    .unwrap();
    assert_eq!(picks, 2);
}

/// The half of the draft `hero_ban` could not record before V7: which side's
/// arsenal a hero was struck out of. A correction has to *move* it, since
/// `correct` replaces the ban set outright rather than merging into it.
#[tokio::test]
async fn a_bans_side_round_trips_through_record_and_moves_on_correct() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let sides = two_sides(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let recorded = post_match(
        &app,
        winter,
        &body(
            1,
            &a_link(),
            sides.clone(),
            games.clone(),
            json!([{ "heroId": bigfoot, "banType": "OPPONENT_BAN", "side": 0 }]),
        ),
    )
    .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());
    let recorded = recorded.json();
    assert_eq!(recorded["bans"][0]["side"], 0);
    assert_eq!(recorded["bans"][0]["banType"], "OPPONENT_BAN");

    let corrected = put_match(
        &app,
        winter,
        recorded["matchId"].as_i64().unwrap(),
        &body(
            1,
            &a_link(),
            sides,
            games,
            json!([{ "heroId": bigfoot, "banType": "OPPONENT_BAN", "side": 1 }]),
        ),
    )
    .await;

    assert_eq!(corrected.status, 200, "{}", corrected.text());
    assert_eq!(corrected.json()["bans"][0]["side"], 1);
}

/// A pre-ban precedes side assignment, so it stores no side -- and `non_null`
/// omits the field rather than emitting a `null`.
#[tokio::test]
async fn a_pre_ban_records_with_no_side() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let bigfoot = app.hero_id("Bigfoot").await;

    let response = post_match(
        &app,
        winter,
        &body(
            1,
            &a_link(),
            two_sides(&app).await,
            alice_beats_robin_hood(&app, map).await,
            json!([{ "heroId": bigfoot, "banType": "PRE_BAN" }]),
        ),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    response.assert_no_json_nulls();
    let ban = &response.json()["bans"][0];
    assert!(ban.get("side").is_none(), "{ban}");
}

// ---------------------------------------------------------------------------
// Player labels
// ---------------------------------------------------------------------------

/// The point of dropping the `player` table: an admin can name a competitor who
/// exists nowhere in the database, or name nobody at all, and the result still
/// records and still scores. A blank label normalises to absent so "nobody" has
/// one representation, not two.
#[tokio::test]
async fn a_player_label_is_free_text_and_a_blank_one_normalises_to_absent() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let drafted = fixture_draft(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let recorded = post_match(
        &app,
        winter,
        &body(
            1,
            &a_link(),
            json!([
                participant(Some("A. N. Other"), &drafted),
                participant(None, &drafted),
            ]),
            games.clone(),
            json!([]),
        ),
    )
    .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());
    recorded.assert_no_json_nulls();
    let recorded = recorded.json();
    assert_eq!(recorded["participants"][0]["playerLabel"], "A. N. Other");
    assert!(
        recorded["participants"][1].get("playerLabel").is_none(),
        "an unattributed side omits the field rather than emitting null"
    );

    let blanked = put_match(
        &app,
        winter,
        recorded["matchId"].as_i64().unwrap(),
        &body(
            1,
            &a_link(),
            json!([
                participant(Some("   "), &drafted),
                participant(Some("Rina Okafor"), &drafted),
            ]),
            games,
            json!([]),
        ),
    )
    .await;

    assert_eq!(blanked.status, 200, "{}", blanked.text());
    let blanked = blanked.json();
    assert!(
        blanked["participants"][0].get("playerLabel").is_none(),
        "a whitespace-only label is stored as absent, not as an empty string"
    );
    assert_eq!(blanked["participants"][1]["playerLabel"], "Rina Okafor");
}

// ---------------------------------------------------------------------------
// The external link
// ---------------------------------------------------------------------------

#[tokio::test]
async fn correcting_a_match_fully_replaces_its_participants_and_games() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let recorded = record_fixture(&app, winter, 1).await;
    let match_id = recorded["matchId"].as_i64().unwrap();
    let king_arthur = app.hero_id("King Arthur").await;
    let medusa = app.hero_id("Medusa").await;

    let corrected = put_match(
        &app,
        winter,
        match_id,
        &body(
            1,
            "https://example.com/bracket/42",
            two_sides(&app).await,
            json!([game(1, map, (king_arthur, 8, true), (medusa, 0, false))]),
            json!([]),
        ),
    )
    .await;

    assert_eq!(corrected.status, 200, "{}", corrected.text());
    let corrected = corrected.json();
    let mut fielded: Vec<&str> = corrected["games"][0]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["heroName"].as_str())
        .collect();
    fielded.sort();
    assert_eq!(fielded, ["King Arthur", "Medusa"]);
    assert_eq!(
        corrected["matchId"].as_i64(),
        Some(match_id),
        "correcting reuses the same match id"
    );
    assert_eq!(corrected["externalLink"], "https://example.com/bracket/42");
}

/// Whitespace around a pasted URL must not buy a second copy of the match: the
/// stored value and the duplicate check see the same string.
#[tokio::test]
async fn an_external_link_is_trimmed_before_it_is_stored_and_compared() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let link = a_link();
    let sides = two_sides(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let recorded = post_match(
        &app,
        winter,
        &body(
            1,
            &format!("  {link}  "),
            sides.clone(),
            games.clone(),
            json!([]),
        ),
    )
    .await;
    assert_eq!(recorded.status, 201, "{}", recorded.text());
    assert_eq!(recorded.json()["externalLink"], link);

    let duplicate = post_match(&app, winter, &body(2, &link, sides, games, json!([]))).await;
    assert_eq!(duplicate.status, 409, "{}", duplicate.text());
}

/// A duplicated match silently double-counts every appearance, win and ban it
/// carries, so the refusal has to *name the match to correct* -- the admin UI
/// links straight to it.
#[tokio::test]
async fn recording_a_second_match_against_a_used_link_is_rejected_and_names_the_first() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let link = a_link();
    let sides = two_sides(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let first = post_match(
        &app,
        winter,
        &body(1, &link, sides.clone(), games.clone(), json!([])),
    )
    .await
    .json();
    let first_id = first["matchId"].as_i64().unwrap();

    let conflict = post_match(&app, winter, &body(2, &link, sides, games, json!([]))).await;

    assert_eq!(conflict.status, 409, "{}", conflict.text());
    let body = conflict.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    let detail = body["detail"].as_str().expect("a detail").to_owned();
    assert!(detail.contains(&first_id.to_string()), "{detail}");
}

/// Uniqueness is scoped to the tournament, matching the importer's own
/// per-tournament duplicate check.
#[tokio::test]
async fn the_same_link_may_be_recorded_in_a_different_tournament() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let summer = app.tournament_id("Summer of Legends").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let link = a_link();
    let sides = two_sides(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let first = post_match(
        &app,
        winter,
        &body(1, &link, sides.clone(), games.clone(), json!([])),
    )
    .await;
    assert_eq!(first.status, 201, "{}", first.text());

    let second = post_match(&app, summer, &body(1, &link, sides, games, json!([]))).await;

    assert_eq!(second.status, 201, "{}", second.text());
    assert_eq!(second.json()["externalLink"], link);
}

#[tokio::test]
async fn correcting_a_match_keeps_its_own_link_but_cannot_steal_another_matchs() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let own_link = a_link();
    let sides = two_sides(&app).await;
    let games = alice_beats_robin_hood(&app, map).await;

    let recorded = post_match(
        &app,
        winter,
        &body(1, &own_link, sides.clone(), games.clone(), json!([])),
    )
    .await
    .json();
    let match_id = recorded["matchId"].as_i64().unwrap();
    let sibling = post_match(
        &app,
        winter,
        &body(2, &a_link(), sides.clone(), games.clone(), json!([])),
    )
    .await
    .json();

    // The ordinary correction path: the row is updated in place, so re-saving
    // it under the link it already holds never meets the unique index.
    let corrected = put_match(
        &app,
        winter,
        match_id,
        &body(3, &own_link, sides.clone(), games.clone(), json!([])),
    )
    .await;
    assert_eq!(corrected.status, 200, "{}", corrected.text());
    assert_eq!(corrected.json()["externalLink"], own_link);
    assert_eq!(corrected.json()["round"], 3);

    let theft = put_match(
        &app,
        winter,
        match_id,
        &body(
            1,
            sibling["externalLink"].as_str().unwrap(),
            sides,
            games,
            json!([]),
        ),
    )
    .await;
    assert_eq!(theft.status, 409, "{}", theft.text());
}

// ---------------------------------------------------------------------------
// Reading one back, and retracting it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_recorded_match_reads_back_by_id() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let recorded = record_fixture(&app, winter, 1).await;
    let match_id = recorded["matchId"].as_i64().unwrap();

    let response = app
        .get_as(
            &format!("/api/admin/tournaments/{winter}/matches/{match_id}"),
            admin(&app).await,
        )
        .await;

    assert_eq!(response.status, 200, "{}", response.text());
    response.assert_no_json_nulls();
    assert_eq!(response.json(), recorded);
}

/// A match belongs to exactly one tournament, and reaching it through another
/// one is a 404 rather than a leak.
#[tokio::test]
async fn a_match_is_not_reachable_through_another_tournament() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let summer = app.tournament_id("Summer of Legends").await;
    let match_id = record_fixture(&app, winter, 1).await["matchId"]
        .as_i64()
        .unwrap();

    let response = app
        .get_as(
            &format!("/api/admin/tournaments/{summer}/matches/{match_id}"),
            admin(&app).await,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(
        response.json()["detail"],
        format!("No match {match_id} in tournament {summer}")
    );
}

#[tokio::test]
async fn deleting_a_match_removes_its_participants_games_bans_and_picks() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;
    let bigfoot = app.hero_id("Bigfoot").await;
    let recorded = post_match(
        &app,
        winter,
        &body(
            1,
            &a_link(),
            two_sides(&app).await,
            alice_beats_robin_hood(&app, map).await,
            json!([{ "heroId": bigfoot, "banType": "SELF_BAN", "side": 0 }]),
        ),
    )
    .await
    .json();
    let match_id = recorded["matchId"].as_i64().unwrap();
    let game_id = recorded["games"][0]["gameId"].as_i64().unwrap();

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/matches/{match_id}"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 204, "{}", response.text());
    assert!(response.body.is_empty(), "204 carries no body");
    assert_eq!(
        count("match_participant", "match_id", match_id, &app).await,
        0
    );
    assert_eq!(count("match_game", "match_id", match_id, &app).await, 0);
    assert_eq!(
        count("match_game_participant", "game_id", game_id, &app).await,
        0
    );
    assert_eq!(count("hero_ban", "match_id", match_id, &app).await, 0);
    assert_eq!(
        count("match_hero_pick", "match_id", match_id, &app).await,
        0
    );

    let mut conn = app.pool().acquire().await.unwrap();
    assert!(
        umfl_server::r#match::query::find_by_id(&mut conn, match_id)
            .await
            .unwrap()
            .is_none()
    );
}

/// A row count for one of the five child tables. Five near-identical queries
/// rather than one built by `format!`, because `sqlx` only checks a literal.
async fn count(table: &str, column: &str, value: i64, app: &TestApp) -> i64 {
    match (table, column) {
        ("match_participant", _) => sqlx::query_scalar!(
            r#"select count(*) as "c!" from match_participant where match_id = $1"#,
            value
        )
        .fetch_one(app.pool())
        .await
        .unwrap(),
        ("match_game", _) => sqlx::query_scalar!(
            r#"select count(*) as "c!" from match_game where match_id = $1"#,
            value
        )
        .fetch_one(app.pool())
        .await
        .unwrap(),
        ("match_game_participant", _) => sqlx::query_scalar!(
            r#"select count(*) as "c!" from match_game_participant where game_id = $1"#,
            value
        )
        .fetch_one(app.pool())
        .await
        .unwrap(),
        ("hero_ban", _) => sqlx::query_scalar!(
            r#"select count(*) as "c!" from hero_ban where match_id = $1"#,
            value
        )
        .fetch_one(app.pool())
        .await
        .unwrap(),
        _ => sqlx::query_scalar!(
            r#"select count(*) as "c!" from match_hero_pick where match_id = $1"#,
            value
        )
        .fetch_one(app.pool())
        .await
        .unwrap(),
    }
}

#[tokio::test]
async fn deleting_an_unknown_match_is_a_404() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "DELETE",
            &format!("/api/admin/tournaments/{winter}/matches/9999999"),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404, "{}", response.text());
}

// ---------------------------------------------------------------------------
// Request validation, as 400s
// ---------------------------------------------------------------------------

/// Every message here is the Hibernate one the client renders verbatim, and the
/// field key is the path Spring's `FieldError.getField()` produced.
#[tokio::test]
async fn a_broken_request_body_is_a_400_naming_every_bad_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = post_match(
        &app,
        winter,
        &json!({
            "round": 0,
            "playedAt": "2026-03-01T18:00:00Z",
            "externalLink": "  ",
            "participants": [],
            "games": [{ "gameNumber": 1, "participants": [] }],
        }),
    )
    .await;

    assert_eq!(response.status, 400, "{}", response.text());
    let fields = &response.json()["fields"];
    assert_eq!(fields["round"], "round must be positive");
    assert_eq!(fields["externalLink"], "externalLink is required");
    assert_eq!(
        fields["participants"],
        "exactly two participants are required"
    );
    // `games` itself is fine -- one game is a legal series -- so only the
    // broken fields *inside* it are reported, each under the path Spring's
    // `FieldError.getField()` produced.
    assert!(fields.get("games").is_none(), "{fields}");
    assert_eq!(fields["games[0].mapId"], "mapId is required");
    assert_eq!(
        fields["games[0].participants"],
        "exactly two sides are required"
    );
}

#[tokio::test]
async fn an_absent_played_at_is_a_400_rather_than_a_500() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let map = map_id(&app, "Baskerville Manor").await;

    let response = post_match(
        &app,
        winter,
        &json!({
            "round": 1,
            "externalLink": a_link(),
            "participants": two_sides(&app).await,
            "games": alice_beats_robin_hood(&app, map).await,
        }),
    )
    .await;

    assert_eq!(response.status, 400, "{}", response.text());
    assert_eq!(
        response.json()["fields"]["playedAt"],
        "playedAt is required"
    );
}

#[tokio::test]
async fn recording_against_an_unknown_tournament_is_a_404() {
    let app = TestApp::spawn().await;
    let map = map_id(&app, "Baskerville Manor").await;

    let response = post_match(
        &app,
        9_999_999,
        &body(
            1,
            &a_link(),
            two_sides(&app).await,
            alice_beats_robin_hood(&app, map).await,
            json!([]),
        ),
    )
    .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["detail"], "No tournament with id 9999999");
}

/// The two layers `AGENTS.md` insists on stay in step: the URL matcher and the
/// per-controller role check are one rule table here, and it answers by role.
#[tokio::test]
async fn a_non_admin_manager_is_refused_and_an_anonymous_request_is_challenged() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let uri = format!("/api/admin/tournaments/{winter}/matches");
    let plain_manager = app.manager("SherlockMain").await.id;

    assert_eq!(app.get(&uri).await.status, 401);
    assert_eq!(app.get_as(&uri, plain_manager).await.status, 403);
    assert_eq!(app.get_as(&uri, admin(&app).await).await.status, 200);
}
