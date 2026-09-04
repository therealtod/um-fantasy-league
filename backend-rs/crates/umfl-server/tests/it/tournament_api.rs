//! The wire contract for `/api/me`, `/api/tournaments/**` and the hero pool.
//!
//! `roster_flow.rs` owns the *rules*; this file owns the *shape* -- field names,
//! status codes, and the fields `default-property-inclusion: non_null` omits.
//! The unchanged Vue frontend working against this backend is the acceptance
//! test, so a renamed key here is as much a break as a wrong number there.

use serde_json::json;

use crate::harness::TestApp;

#[tokio::test]
async fn the_lobby_lists_every_tournament_in_start_date_order() {
    let app = TestApp::spawn().await;

    let response = app.get("/api/tournaments").await;
    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();

    let body = response.json();
    let names: Vec<&str> = body
        .as_array()
        .expect("an array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Summer of Legends",
            "Winter of Champions",
            "Spring of Myths"
        ],
        "ordered by start_date, not by name or id"
    );
}

/// Every field `types.ts` declares, spelled the way it declares it.
#[tokio::test]
async fn a_tournament_carries_the_fields_the_frontend_declares() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app.get(&format!("/api/tournaments/{winter}")).await;
    assert_eq!(response.status, 200);
    let body = response.json();

    assert_eq!(body["id"], winter);
    assert_eq!(body["name"], "Winter of Champions");
    assert_eq!(body["format"], "ARSENAL");
    assert_eq!(body["status"], "REGISTRATION_OPEN");
    assert_eq!(body["startDate"], "2026-08-14");
    assert_eq!(body["capacity"], 64);
    assert_eq!(body["rosterSize"], 3);
    assert_eq!(body["creditGrant"], 10_000);
    assert_eq!(body["acceptsRegistration"], true);
    assert_eq!(body["enrolled"], 0, "Winter starts with no entries");

    // Both are null in the database, so both are *absent* rather than null.
    assert!(body.get("endDate").is_none(), "{body}");
    assert!(body.get("myEntryStatus").is_none(), "{body}");
    response.assert_no_json_nulls();
}

/// `endDate` is present when the row has one, so its absence above is
/// `non_null` at work rather than the field having been dropped in the port.
#[tokio::test]
async fn a_finished_tournament_reports_the_day_it_ended() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id("Summer of Legends").await;

    let body = app.get(&format!("/api/tournaments/{summer}")).await.json();
    assert_eq!(body["endDate"], "2026-06-07");
    assert_eq!(body["status"], "COMPLETED");
    assert_eq!(body["acceptsRegistration"], false);
}

#[tokio::test]
async fn the_status_filter_narrows_the_list() {
    let app = TestApp::spawn().await;

    let body = app.get("/api/tournaments?status=SCHEDULED").await.json();
    let names: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Spring of Myths"]);
}

#[tokio::test]
async fn an_unknown_tournament_is_a_404_naming_it() {
    let app = TestApp::spawn().await;

    let response = app.get("/api/tournaments/999999").await;
    assert_eq!(response.status, 404);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/problem+json")
    );
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/not-found");
    assert_eq!(body["detail"], "No tournament with id 999999");
    assert_eq!(body["instance"], "/api/tournaments/999999");
}

/// `handleTypeMismatch`'s message: before the base class went back in, an
/// unparseable path variable answered 500.
#[tokio::test]
async fn an_unparseable_tournament_id_is_a_400_naming_the_variable() {
    let app = TestApp::spawn().await;

    let response = app.get("/api/tournaments/not-a-number").await;
    assert_eq!(response.status, 400);
    assert_eq!(
        response.json()["detail"],
        "Failed to convert 'id' with value: 'not-a-number'"
    );
}

#[tokio::test]
async fn the_hero_pool_is_priced_for_the_tournament_that_was_asked_about() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let summer = app.tournament_id("Summer of Legends").await;

    let response = app.get(&format!("/api/tournaments/{winter}/heroes")).await;
    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();

    let body = response.json();
    let heroes = body.as_array().unwrap();
    assert_eq!(heroes.len(), 12);
    assert_eq!(heroes[0]["name"], "Medusa", "COST is the default sort");
    assert_eq!(heroes[0]["cost"], 5_600);

    // The same hero, a different price, one tournament over -- cost is
    // tournament-scoped and the seed proves it rather than asserting it.
    let summer_body = app
        .get(&format!("/api/tournaments/{summer}/heroes"))
        .await
        .json();
    let medusa = summer_body
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "Medusa")
        .expect("Medusa is in Summer's pool too")
        .clone();
    assert_eq!(medusa["cost"], 5_100);
}

#[tokio::test]
async fn the_hero_pool_takes_a_sort_and_a_search() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let by_name = app
        .get(&format!("/api/tournaments/{winter}/heroes?sort=NAME"))
        .await
        .json();
    assert_eq!(by_name[0]["name"], "Achilles");

    let searched = app
        .get(&format!("/api/tournaments/{winter}/heroes?search=ho"))
        .await
        .json();
    let names: Vec<&str> = searched
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{searched}");
    assert!(names.contains(&"Robin Hood"), "{searched}");
    assert!(names.contains(&"Sherlock Holmes"), "{searched}");
}

/// An unknown tournament 404s rather than answering an empty pool, which is
/// what `HeroController`'s `requireTournament` call is there for.
#[tokio::test]
async fn the_hero_pool_of_an_unknown_tournament_is_a_404_not_an_empty_list() {
    let app = TestApp::spawn().await;

    let response = app.get("/api/tournaments/999999/heroes").await;
    assert_eq!(response.status, 404);
    assert_eq!(response.json()["detail"], "No tournament with id 999999");
}

#[tokio::test]
async fn me_returns_the_four_fields_the_top_bar_reads() {
    let app = TestApp::spawn().await;
    let manager = app.manager("NeonStrategist").await;

    let response = app.get_as("/api/me", manager.id).await;
    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();

    assert_eq!(
        response.json(),
        json!({
            "id": manager.id,
            "handle": "NeonStrategist",
            "displayName": "Neon Strategist",
            "isAdmin": true,
        }),
        "authUserId is not on the wire and must not appear"
    );
}

/// The whole Roster Builder round trip over HTTP: register, draft, lock.
#[tokio::test]
async fn the_roster_round_trip_answers_201_then_200_and_flips_lockable() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let entries = format!("/api/tournaments/{winter}/entries");

    let created = app.send_as("POST", &entries, manager.id, None).await;
    assert_eq!(created.status, 201);
    created.assert_no_json_nulls();
    let body = created.json();
    assert_eq!(body["tournamentId"], winter);
    assert_eq!(body["tournamentName"], "Winter of Champions");
    assert_eq!(body["status"], "DRAFT");
    assert_eq!(body["locked"], false);
    assert_eq!(body["rosterSize"], 3);
    assert_eq!(body["heroes"], json!([]));
    assert_eq!(body["budget"]["spent"], 0);
    assert_eq!(body["budget"]["creditGrant"], 10_000);
    assert_eq!(body["budget"]["remaining"], 10_000);
    assert_eq!(body["budget"]["utilisation"], 0.0);
    assert_eq!(body["lockable"], false, "an empty roster cannot lock");
    assert!(
        body.get("lockedAt").is_none(),
        "deviation (d): an unset lockedAt is absent, not null"
    );

    let picks = app.hero_ids(&["Alice", "Robin Hood", "Bigfoot"]).await;
    let drafted = app
        .send_as(
            "PUT",
            &format!("{entries}/me/slots"),
            manager.id,
            Some(&json!({ "heroIds": picks })),
        )
        .await;
    assert_eq!(drafted.status, 200);
    let body = drafted.json();
    assert_eq!(body["budget"]["spent"], 9_400);
    assert_eq!(body["budget"]["remaining"], 600);
    assert_eq!(body["lockable"], true);
    let ids: Vec<i64> = body["heroes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, picks, "the response keeps slot order");

    let locked = app
        .send_as("POST", &format!("{entries}/me/lock"), manager.id, None)
        .await;
    assert_eq!(locked.status, 200);
    let body = locked.json();
    assert_eq!(body["status"], "LOCKED");
    assert_eq!(body["locked"], true);
    assert!(
        body["lockedAt"].as_str().unwrap().ends_with('Z'),
        "a Java Instant renders a literal Z, not +00:00: {body}"
    );

    // And the roster reads back the same way it was written.
    let reloaded = app.get_as(&format!("{entries}/me"), manager.id).await;
    assert_eq!(reloaded.status, 200);
    assert_eq!(reloaded.json()["status"], "LOCKED");
}

#[tokio::test]
async fn a_manager_with_no_entry_here_is_a_404_saying_so() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;

    let response = app
        .get_as(&format!("/api/tournaments/{winter}/entries/me"), manager.id)
        .await;
    assert_eq!(response.status, 404);
    assert_eq!(
        response.json()["detail"],
        format!("You are not registered for tournament {winter}.")
    );
}

/// The 422 the Roster Builder highlights from: a `violations` array, every
/// broken rule at once rather than one per round trip.
#[tokio::test]
async fn a_broken_roster_rule_is_a_422_carrying_every_violation() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("ArthurianLegend").await;
    let entries = format!("/api/tournaments/{winter}/entries");
    app.send_as("POST", &entries, manager.id, None).await;

    let alice = app.hero_id("Alice").await;
    let response = app
        .send_as(
            "PUT",
            &format!("{entries}/me/slots"),
            manager.id,
            // Four picks against a roster size of three, all of them the same:
            // TOO_MANY_PICKS and DUPLICATE_HERO together.
            Some(&json!({ "heroIds": [alice, alice, alice, alice] })),
        )
        .await;

    assert_eq!(response.status, 422);
    let body = response.json();
    assert_eq!(
        body["type"],
        "https://umfl.dev/problems/roster-rule-violation"
    );
    assert_eq!(body["title"], "Roster rules violated");
    let rules: Vec<&str> = body["violations"]
        .as_array()
        .expect("a violations array")
        .iter()
        .map(|v| v["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["TOO_MANY_PICKS", "DUPLICATE_HERO"]);
}

/// `heroIds` is required, and the field key is the **camelCase** name -- the
/// name the client knows the field by.
#[tokio::test]
async fn a_missing_hero_ids_is_a_400_naming_the_field_the_way_the_client_does() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;
    let entries = format!("/api/tournaments/{winter}/entries");
    app.send_as("POST", &entries, manager.id, None).await;

    let response = app
        .send_as(
            "PUT",
            &format!("{entries}/me/slots"),
            manager.id,
            Some(&json!({})),
        )
        .await;

    assert_eq!(response.status, 400);
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/validation-failed");
    assert_eq!(body["detail"], "One or more fields failed validation.");
    assert_eq!(body["fields"]["heroIds"], "heroIds is required");
}

/// The lobby personalises each card for the caller, and leaves the field out
/// entirely for everyone else.
#[tokio::test]
async fn my_entry_status_appears_only_for_the_manager_who_has_one() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    app.send_as(
        "POST",
        &format!("/api/tournaments/{winter}/entries"),
        manager.id,
        None,
    )
    .await;

    let mine = app.get_as("/api/tournaments", manager.id).await.json();
    let card = mine
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == winter)
        .cloned()
        .unwrap();
    assert_eq!(card["myEntryStatus"], "DRAFT");
    assert_eq!(card["enrolled"], 1);

    let anonymous = app.get("/api/tournaments").await;
    anonymous.assert_no_json_nulls();
    let card = anonymous
        .json()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == winter)
        .cloned()
        .unwrap();
    assert!(card.get("myEntryStatus").is_none(), "{card}");
    assert_eq!(card["enrolled"], 1, "the count is not personalised");
}
