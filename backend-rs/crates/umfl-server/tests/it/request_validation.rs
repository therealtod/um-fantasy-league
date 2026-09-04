//! Every `ValidJson` request body, hit with `{}`, must 400 naming *exactly*
//! the fields its garde rules require -- no more, no fewer.
//!
//! Forty-odd sites across `crates/umfl-server/src/` follow the same triad: a
//! request field typed `Option<T>`, a garde rule that makes it required, and a
//! `.expect("validated as present")` where a service unwraps it. Nothing but
//! convention connects the garde rule to the `.expect` -- add a field with the
//! `Option` type and forget the rule, and a missing field stops being a 400
//! naming it and becomes a **500** from the `.expect` instead. That is a real,
//! already-observed shape in this codebase: `SetHeroCostRequest`/
//! `HeroPoolEntryRequest.cost` had exactly this gap, found by hand -- the
//! identical shape already fixed once for `capacity`/`rosterSize`/
//! `creditGrant`. Nothing stops a fourth instance appearing silently.
//!
//! This module is the check that convention alone can't be. **Asserting the
//! full field-*set*, not merely that `fields` is non-empty, is the entire
//! point**: a field that gains an `Option` type but no garde rule does not
//! show up as an *extra* key (there is nothing to report it) -- it shows up
//! as a **missing** one, and only a set comparison catches an absence. A
//! looser "some fields were reported" assertion would pass right through the
//! defect this module exists to catch.
//!
//! Two endpoints are deliberately **not** in the table below, for a different
//! reason than "can't reach validation": `AddHeroesToPoolRequest.heroes` and
//! `AddMapsToPoolRequest.map_ids` are `Option<Vec<_>>` with no `required(...)`
//! rule at all -- `{}` validates cleanly and the handler deliberately reads
//! the field as an empty batch (`.unwrap_or_default()`). There is no
//! `.expect(...)` on either field, so there is no defect shape here for this
//! module to guard -- see `an_empty_batch_pool_body_is_accepted_as_an_empty_batch`
//! below, which pins that this is a deliberate, working design rather than an
//! oversight.

use std::collections::BTreeSet;

use serde_json::json;

use crate::harness::TestApp;

/// *NeonStrategist* is the seed's only `is_admin` manager. Used for every case
/// here, including the one non-admin route (`PUT .../entries/me/slots`),
/// since that route only demands `authenticated()` and an admin satisfies
/// that too.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

/// Reused verbatim from `match_admin.rs` so the `.sqlx/` cache entry is
/// shared rather than duplicated.
async fn map_id(app: &TestApp, name: &str) -> i64 {
    sqlx::query_scalar!("select id from game_maps where name = $1", name)
        .fetch_one(app.pool())
        .await
        .unwrap_or_else(|e| panic!("no board {name}: {e}"))
}

/// Winter of Champions' one seeded rule set -- see `scoring_admin.rs`.
async fn rule_set_id(app: &TestApp, tournament_id: i64) -> i64 {
    sqlx::query_scalar!(
        "select id from scoring_rule_sets where tournament_id = $1",
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .unwrap_or_else(|e| panic!("no seeded rule set for tournament {tournament_id}: {e}"))
}

/// Records one throwaway match against Winter of Champions -- which the seed
/// deliberately leaves with zero matches, see `match_admin.rs` -- purely so
/// `PUT .../matches/{matchId}` has a real id to correct. The body validation
/// under test runs at the `ValidJson` extractor, ahead of anything the
/// handler itself does, so this match's *content* is beside the point; it
/// only has to exist.
async fn a_recorded_match_id(app: &TestApp, tournament_id: i64, admin_id: i64) -> i64 {
    let alice = app.hero_id("Alice").await;
    let robin_hood = app.hero_id("Robin Hood").await;
    let map = map_id(app, "Baskerville Manor").await;
    let body = json!({
        "round": 1,
        "playedAt": "2026-03-01T18:00:00Z",
        "externalLink": format!("https://example.com/match/{}", uuid::Uuid::new_v4()),
        "participants": [
            { "playerLabel": "Side A", "draftedHeroIds": [alice] },
            { "playerLabel": "Side B", "draftedHeroIds": [robin_hood] },
        ],
        "games": [{
            "gameNumber": 1,
            "mapId": map,
            "participants": [
                { "heroId": alice, "healthRemaining": 6, "isWinner": true },
                { "heroId": robin_hood, "healthRemaining": 0, "isWinner": false },
            ],
        }],
        "bans": [],
    });
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{tournament_id}/matches"),
            admin_id,
            Some(&body),
        )
        .await;
    assert_eq!(
        response.status,
        201,
        "fixture match for request_validation.rs: {}",
        response.text()
    );
    response.json()["matchId"]
        .as_i64()
        .expect("a saved match has an id")
}

/// One endpoint under test: an HTTP method and path, the manager sending it,
/// and the *exact* set of camelCase field names its DTO's garde rules are
/// supposed to report against `{}`.
///
/// A table rather than N near-identical `#[tokio::test]`s, matching how this
/// codebase writes fixture tables elsewhere (e.g. `HeroSort`, the coefficient
/// tables in `scoring_admin.rs`) -- adding an endpoint later is one row here,
/// not a new function.
struct Case {
    name: &'static str,
    method: &'static str,
    uri: String,
    manager_id: i64,
    fields: &'static [&'static str],
}

#[tokio::test]
async fn an_empty_body_fails_validation_naming_exactly_the_required_fields() {
    let app = TestApp::spawn().await;
    let admin_id = admin(&app).await;
    let winter = app.tournament_id("Winter of Champions").await;
    let hero_id = app.hero_id("Alice").await;
    let map_id = map_id(&app, "Baskerville Manor").await;
    let rule_set_id = rule_set_id(&app, winter).await;
    let match_id = a_recorded_match_id(&app, winter, admin_id).await;

    let cases = vec![
        Case {
            name: "create hero",
            method: "POST",
            uri: "/api/admin/heroes".to_owned(),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "update hero",
            method: "PUT",
            uri: format!("/api/admin/heroes/{hero_id}"),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "set hero pool cost",
            method: "PUT",
            uri: format!("/api/admin/tournaments/{winter}/heroes/{hero_id}"),
            manager_id: admin_id,
            fields: &["cost"],
        },
        Case {
            name: "create scoring rule set",
            method: "POST",
            uri: format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "update scoring rule set",
            method: "PUT",
            uri: format!("/api/admin/tournaments/{winter}/scoring-rule-sets/{rule_set_id}"),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "set roster slots",
            method: "PUT",
            uri: format!("/api/tournaments/{winter}/entries/me/slots"),
            manager_id: admin_id,
            fields: &["heroIds"],
        },
        Case {
            name: "create tournament",
            method: "POST",
            uri: "/api/admin/tournaments".to_owned(),
            manager_id: admin_id,
            fields: &[
                "name",
                "format",
                "status",
                "startDate",
                "capacity",
                "rosterSize",
                "creditGrant",
            ],
        },
        Case {
            name: "update tournament",
            method: "PUT",
            uri: format!("/api/admin/tournaments/{winter}"),
            manager_id: admin_id,
            fields: &[
                "name",
                "format",
                "status",
                "startDate",
                "capacity",
                "rosterSize",
                "creditGrant",
            ],
        },
        Case {
            name: "create map",
            method: "POST",
            uri: "/api/admin/maps".to_owned(),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "update map",
            method: "PUT",
            uri: format!("/api/admin/maps/{map_id}"),
            manager_id: admin_id,
            fields: &["name"],
        },
        Case {
            name: "record match",
            method: "POST",
            uri: format!("/api/admin/tournaments/{winter}/matches"),
            manager_id: admin_id,
            fields: &["round", "playedAt", "externalLink"],
        },
        Case {
            name: "correct match",
            method: "PUT",
            uri: format!("/api/admin/tournaments/{winter}/matches/{match_id}"),
            manager_id: admin_id,
            fields: &["round", "playedAt", "externalLink"],
        },
        Case {
            name: "import match",
            method: "POST",
            uri: format!("/api/admin/tournaments/{winter}/matches/import"),
            manager_id: admin_id,
            fields: &["sourceUrl"],
        },
    ];

    for case in cases {
        let response = app
            .send_as(case.method, &case.uri, case.manager_id, Some(&json!({})))
            .await;

        assert_eq!(
            response.status,
            400,
            "{}: expected 400, got {} -- {}",
            case.name,
            response.status,
            response.text()
        );
        let body = response.json();
        assert_eq!(
            body["type"], "https://umfl.dev/problems/validation-failed",
            "{}: {body}",
            case.name
        );

        let actual: BTreeSet<String> = body["fields"]
            .as_object()
            .unwrap_or_else(|| panic!("{}: `fields` is not an object -- {body}", case.name))
            .keys()
            .cloned()
            .collect();
        let expected: BTreeSet<String> = case.fields.iter().map(|f| (*f).to_owned()).collect();

        // The exact-set comparison, not a subset/non-empty check: a field
        // that acquired an `Option` type but no garde rule is silently
        // *absent* from `actual` here, and only this assertion notices.
        assert_eq!(
            actual, expected,
            "{}: reported field set does not match the DTO's required fields\nfull body: {body:#}",
            case.name
        );
    }
}

/// Pins the two endpoints excluded from the table above as a deliberate
/// design, not an oversight: `AddHeroesToPoolRequest.heroes` and
/// `AddMapsToPoolRequest.map_ids` are `Option<Vec<_>>` with no `required(...)`
/// rule, so `{}` validates cleanly -- the handler deliberately reads it as an
/// empty batch. Neither field is ever `.expect()`-unwrapped, so there is no
/// `Option` + missing-rule + `.expect()` triad here for this module to guard
/// -- an empty body is genuinely valid input, not a gap.
#[tokio::test]
async fn an_empty_batch_pool_body_is_accepted_as_an_empty_batch() {
    let app = TestApp::spawn().await;
    let admin_id = admin(&app).await;
    let winter = app.tournament_id("Winter of Champions").await;

    let hero_batch = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/heroes"),
            admin_id,
            Some(&json!({})),
        )
        .await;
    assert_eq!(hero_batch.status, 200, "{}", hero_batch.text());
    assert_eq!(hero_batch.json().as_array().map(Vec::len), Some(0));

    let map_batch = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/maps"),
            admin_id,
            Some(&json!({})),
        )
        .await;
    assert_eq!(map_batch.status, 200, "{}", map_batch.text());
    assert_eq!(map_batch.json().as_array().map(Vec::len), Some(0));
}
