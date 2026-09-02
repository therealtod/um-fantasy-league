//! Scoring rule sets, through `/api/admin/tournaments/{id}/scoring-rule-sets`.
//!
//! Oracle: `scoring/AdminScoringServiceIntegrationTest.kt`, driven over HTTP
//! rather than against the service, so the DTO shape and the status codes are
//! checked by the same test that checks the rule.
//!
//! The seed's numbers are asserted exactly, as `AGENTS.md` requires: Winter of
//! Champions ships with one active rule set, *Season 2026 Standard*, whose
//! eight coefficients include the deliberately unimplemented `CROWD_FAVOURITE`.

use serde_json::{Value, json};

use crate::harness::TestApp;

/// *NeonStrategist* is the seed's only `is_admin` manager.
async fn admin(app: &TestApp) -> i64 {
    app.manager("NeonStrategist").await.id
}

fn coefficient(metric: &str, weight: f64, sort_order: i32) -> Value {
    json!({ "metric": metric, "coefficient": weight, "sortOrder": sort_order })
}

async fn create_rule_set(app: &TestApp, tournament_id: i64, body: &Value) -> Value {
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{tournament_id}/scoring-rule-sets"),
            admin(app).await,
            Some(body),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()
}

/// The active rule set's name, straight out of the table -- the assertion
/// `ScoringRuleSetQuery.activeRules(...).name` makes in the Kotlin.
async fn active_rule_set_name(app: &TestApp, tournament_id: i64) -> String {
    sqlx::query_scalar!(
        "select name from scoring_rule_sets where tournament_id = $1 and is_active",
        tournament_id
    )
    .fetch_one(app.pool())
    .await
    .expect("exactly one active rule set")
}

#[tokio::test]
async fn lists_a_tournaments_rule_sets_active_one_first() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;

    let response = app
        .get_as(
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
        )
        .await;
    assert_eq!(response.status, 200);
    response.assert_no_json_nulls();

    let listed = response.json();
    let listed = listed.as_array().expect("an array");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["name"], "Season 2026 Standard");
    assert_eq!(listed[0]["isActive"], true);
    assert_eq!(listed[1]["name"], "Retuned Weights");
    assert_eq!(listed[1]["isActive"], false);
}

#[tokio::test]
async fn listing_surfaces_the_same_unknown_metric_warning_as_create_does() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Experimental Weights",
            "coefficients": [coefficient("CROWD_FAVOURITE", 5.0, 0)],
            "activate": false,
        }),
    )
    .await;

    let listed = app
        .get_as(
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
        )
        .await
        .json();
    let experimental = listed
        .as_array()
        .expect("an array")
        .iter()
        .find(|rs| rs["name"] == "Experimental Weights")
        .expect("the rule set just created");
    assert_eq!(experimental["warnings"], json!(["CROWD_FAVOURITE"]));

    // The seeded set prices CROWD_FAVOURITE too, and warns about exactly it.
    let seeded = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|rs| rs["name"] == "Season 2026 Standard")
        .expect("the seeded rule set");
    assert_eq!(seeded["warnings"], json!(["CROWD_FAVOURITE"]));
}

/// The application always writes `is_active`, so only a hand-written INSERT
/// sees the schema default. With the old `default true` this statement tripped
/// `uq_scoring_rule_set_active` against the seeded active rule set instead of
/// writing a draft.
#[tokio::test]
async fn an_insert_that_omits_is_active_lands_inactive_not_active() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    sqlx::query!(
        "insert into scoring_rule_sets (tournament_id, name) values ($1, 'Hand-Written Draft')",
        winter
    )
    .execute(app.pool())
    .await
    .expect("the schema default is false, so this does not collide");

    let is_active = sqlx::query_scalar!(
        "select is_active from scoring_rule_sets where tournament_id = $1 and name = $2",
        winter,
        "Hand-Written Draft"
    )
    .fetch_one(app.pool())
    .await
    .unwrap();
    assert!(!is_active);
    assert_eq!(
        active_rule_set_name(&app, winter).await,
        "Season 2026 Standard"
    );
}

#[tokio::test]
async fn creates_a_new_inactive_rule_set_alongside_the_seeded_active_one() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;

    assert_eq!(created["isActive"], false);
    assert_eq!(created["tournamentId"], winter);
    assert_eq!(created["coefficients"].as_array().unwrap().len(), 1);
    assert_eq!(created["warnings"], json!([]));
    // The seeded rule set is still the one standings actually uses.
    assert_eq!(
        active_rule_set_name(&app, winter).await,
        "Season 2026 Standard"
    );
}

/// Every field `types.ts` declares, spelled the way it declares it -- and
/// `coefficient` as a JSON *number*, which is what `http::big_decimal` is for.
#[tokio::test]
async fn a_rule_set_carries_the_fields_the_frontend_declares() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .get_as(
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
        )
        .await;
    let listed = response.json();
    let seeded = &listed.as_array().expect("an array")[0];

    assert!(seeded["id"].is_i64());
    assert_eq!(seeded["tournamentId"], winter);
    assert_eq!(seeded["name"], "Season 2026 Standard");
    assert_eq!(seeded["isActive"], true);

    // Column order is `sort_order`, which is the leaderboard's left-to-right.
    let metrics: Vec<&str> = seeded["coefficients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["metric"].as_str().unwrap())
        .collect();
    assert_eq!(
        metrics,
        [
            "WIN",
            "HEALTH_REMAINING",
            "HEALTH_DIFFERENTIAL",
            "SHUTOUT",
            "SELF_BAN",
            "OPPONENT_BAN",
            "APPEARANCE",
            "CROWD_FAVOURITE",
        ]
    );

    // `numeric(10,4)` keeps its scale on the wire: `0.7500`, not `0.75` and
    // not the string `"0.7500"`.
    let health_remaining = &seeded["coefficients"][1];
    assert_eq!(health_remaining["metric"], "HEALTH_REMAINING");
    assert!(health_remaining["coefficient"].is_number(), "not a string");
    assert_eq!(health_remaining["sortOrder"], 1);
    assert!(
        response.text().contains(r#""coefficient":0.7500"#),
        "trailing zeros are part of the body: {}",
        response.text()
    );
}

#[tokio::test]
async fn creating_with_an_unknown_metric_warns_but_does_not_reject() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Experimental Weights",
            "coefficients": [coefficient("CROWD_FAVOURITE", 5.0, 0)],
            "activate": false,
        }),
    )
    .await;

    assert_eq!(created["warnings"], json!(["CROWD_FAVOURITE"]));
}

#[tokio::test]
async fn creating_a_rule_set_with_a_name_already_used_in_this_tournament_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
            Some(&json!({
                "name": "Season 2026 Standard",
                "coefficients": [coefficient("WIN", 10.0, 0)],
            })),
        )
        .await;

    assert_eq!(response.status, 409);
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/conflict");
    assert_eq!(
        body["detail"],
        format!(
            "A scoring rule set named 'Season 2026 Standard' already exists for tournament {winter}."
        )
    );
}

#[tokio::test]
async fn activating_a_new_rule_set_deactivates_the_previously_active_one() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;
    let rule_set_id = created["id"].as_i64().unwrap();

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets/{rule_set_id}/activate"),
            admin(&app).await,
            None,
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let activated = response.json();
    assert_eq!(activated["isActive"], true);
    // `ScoringRuleSetDto.from(ruleSet)` defaults `warnings` to empty.
    assert_eq!(activated["warnings"], json!([]));

    assert_eq!(active_rule_set_name(&app, winter).await, "Retuned Weights");
    let seeded_active = sqlx::query_scalar!(
        "select is_active from scoring_rule_sets where tournament_id = $1 and name = $2",
        winter,
        "Season 2026 Standard"
    )
    .fetch_one(app.pool())
    .await
    .unwrap();
    assert!(!seeded_active);

    // The weight the newly-active set prices WIN at, read back at the column's
    // own scale.
    let weight = sqlx::query_scalar!(
        "select c.coefficient from scoring_coefficients c
         join scoring_rule_sets rs on rs.id = c.rule_set_id
         where rs.tournament_id = $1 and rs.is_active and c.metric = 'WIN'",
        winter
    )
    .fetch_one(app.pool())
    .await
    .unwrap();
    assert_eq!(weight.to_string(), "12.0000");
}

/// A boolean flip must not delete-and-reinsert the owned children -- which is
/// why activation goes through two targeted statements rather than an
/// aggregate write.
#[tokio::test]
async fn activating_leaves_both_rule_sets_coefficient_rows_untouched() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;
    let rule_set_id = created["id"].as_i64().unwrap();

    let before = coefficient_ids(&app, winter).await;
    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets/{rule_set_id}/activate"),
            admin(&app).await,
            None,
        )
        .await;
    assert_eq!(response.status, 200);

    assert_eq!(before, coefficient_ids(&app, winter).await);
}

async fn coefficient_ids(app: &TestApp, tournament_id: i64) -> Vec<(i64, i64)> {
    sqlx::query!(
        "select c.rule_set_id, c.id from scoring_coefficients c
         join scoring_rule_sets rs on rs.id = c.rule_set_id
         where rs.tournament_id = $1 order by c.rule_set_id, c.id",
        tournament_id
    )
    .fetch_all(app.pool())
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.rule_set_id, r.id))
    .collect()
}

#[tokio::test]
async fn create_can_activate_immediately() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Immediate Activation",
            "coefficients": [coefficient("WIN", 9.0, 0)],
            "activate": true,
        }),
    )
    .await;

    assert_eq!(created["isActive"], true);
    assert_eq!(
        active_rule_set_name(&app, winter).await,
        "Immediate Activation"
    );
}

#[tokio::test]
async fn updates_a_rule_sets_coefficients() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;
    let rule_set_id = created["id"].as_i64().unwrap();

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets/{rule_set_id}"),
            admin(&app).await,
            Some(&json!({
                "name": "Retuned Weights",
                "coefficients": [coefficient("WIN", 15.0, 0), coefficient("BAN", 3.0, 1)],
            })),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let updated = response.json();
    let metrics: Vec<&str> = updated["coefficients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["metric"].as_str().unwrap())
        .collect();
    assert_eq!(metrics, ["WIN", "BAN"]);
    // `BAN` is not a metric any extractor implements; `SELF_BAN` and
    // `OPPONENT_BAN` are.
    assert_eq!(updated["warnings"], json!(["BAN"]));
}

/// Both of these used to reach the database and come back as the generic 409
/// data-integrity backstop -- the unique `(rule_set_id, metric)` index for the
/// first, the metric format CHECK for the second. `ScoringRuleSetPolicy` names
/// them before the insert.
#[tokio::test]
async fn creating_with_the_same_metric_twice_is_a_named_rule_violation() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
            Some(&json!({
                "name": "Double Weighted",
                "coefficients": [coefficient("WIN", 1.0, 0), coefficient("WIN", 2.0, 1)],
            })),
        )
        .await;

    assert_eq!(response.status, 422);
    let body = response.json();
    assert_eq!(
        body["type"],
        "https://umfl.dev/problems/scoring-rule-violation"
    );
    assert_eq!(body["violations"][0]["rule"], "DUPLICATE_METRIC");
    assert_eq!(
        body["violations"][0]["message"],
        "Metric(s) priced more than once: 'WIN'."
    );
    assert_eq!(body["violations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn updating_with_a_malformed_metric_is_a_named_rule_violation() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let created = create_rule_set(
        &app,
        winter,
        &json!({
            "name": "Retuned Weights",
            "coefficients": [coefficient("WIN", 12.0, 0)],
            "activate": false,
        }),
    )
    .await;
    let rule_set_id = created["id"].as_i64().unwrap();

    let response = app
        .send_as(
            "PUT",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets/{rule_set_id}"),
            admin(&app).await,
            Some(&json!({
                "name": "Retuned Weights",
                "coefficients": [coefficient("win-rate", 1.0, 0)],
            })),
        )
        .await;

    assert_eq!(response.status, 422);
    let body = response.json();
    assert_eq!(body["violations"][0]["rule"], "MALFORMED_METRIC");
    assert_eq!(
        body["violations"][0]["message"],
        "Metric name(s) must be letters, digits and underscores starting with a letter: 'WIN-RATE'."
    );
}

#[tokio::test]
async fn an_unknown_tournament_is_a_404_and_an_unknown_rule_set_names_both_ids() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let admin_id = admin(&app).await;

    let missing_tournament = app
        .get_as("/api/admin/tournaments/999999/scoring-rule-sets", admin_id)
        .await;
    assert_eq!(missing_tournament.status, 404);
    assert_eq!(
        missing_tournament.json()["detail"],
        "No tournament with id 999999"
    );

    let missing_rule_set = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets/999999/activate"),
            admin_id,
            None,
        )
        .await;
    assert_eq!(missing_rule_set.status, 404);
    assert_eq!(
        missing_rule_set.json()["detail"],
        format!("No scoring rule set 999999 for tournament {winter}")
    );
}

/// A rule set belonging to another tournament is a 404 here, not someone
/// else's row returned under this tournament's URL.
#[tokio::test]
async fn a_rule_set_from_another_tournament_is_not_reachable_through_this_one() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let summer = app.tournament_id("Summer of Legends").await;
    let summers_rule_set = sqlx::query_scalar!(
        "select id from scoring_rule_sets where tournament_id = $1",
        summer
    )
    .fetch_one(app.pool())
    .await
    .unwrap();

    let response = app
        .send_as(
            "POST",
            &format!(
                "/api/admin/tournaments/{winter}/scoring-rule-sets/{summers_rule_set}/activate"
            ),
            admin(&app).await,
            None,
        )
        .await;

    assert_eq!(response.status, 404);
    assert_eq!(
        active_rule_set_name(&app, winter).await,
        "Season 2026 Standard"
    );
}

/// `@NotBlank` and `@Size(min = 1)`, with the messages the client renders and
/// the field paths `handleMethodArgumentNotValid` produced.
#[tokio::test]
async fn a_broken_request_body_is_a_400_naming_every_bad_field() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
            Some(&json!({ "name": "   ", "coefficients": [{ "sortOrder": 0 }] })),
        )
        .await;

    assert_eq!(response.status, 400);
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/validation-failed");
    assert_eq!(body["fields"]["name"], "name is required");
    assert_eq!(
        body["fields"]["coefficients[0].metric"],
        "metric is required"
    );
    assert_eq!(
        body["fields"]["coefficients[0].coefficient"],
        "coefficient is required"
    );
}

/// A coefficient that is present but not a number at all -- neither absent
/// nor an explicit `null` -- is not something `garde` should ever see: it
/// never becomes "coefficient is required". Jackson can't parse a `BigDecimal`
/// out of `"abc"` either, and answers the same way: a 400 naming the
/// malformed-body problem type, not the validation one, with the same
/// `"Failed to read request"` sentence `AppJson` copies from
/// `HttpMessageNotReadableException`.
#[tokio::test]
async fn a_non_numeric_coefficient_token_is_a_malformed_body_not_a_validation_failure() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let response = app
        .send_as(
            "POST",
            &format!("/api/admin/tournaments/{winter}/scoring-rule-sets"),
            admin(&app).await,
            Some(&json!({
                "name": "Retuned Weights",
                "coefficients": [{ "metric": "WIN", "coefficient": "abc", "sortOrder": 0 }],
            })),
        )
        .await;

    assert_eq!(response.status, 400, "{}", response.text());
    let body = response.json();
    assert_eq!(body["type"], "https://umfl.dev/problems/bad-request");
    assert_eq!(body["detail"], "Failed to read request");
}

#[tokio::test]
async fn an_empty_coefficient_list_is_rejected_but_an_absent_one_is_not() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let admin_id = admin(&app).await;
    let url = format!("/api/admin/tournaments/{winter}/scoring-rule-sets");

    let empty = app
        .send_as(
            "POST",
            &url,
            admin_id,
            Some(&json!({ "name": "Empty", "coefficients": [] })),
        )
        .await;
    assert_eq!(empty.status, 400);
    assert_eq!(
        empty.json()["fields"]["coefficients"],
        "at least one coefficient is required"
    );

    // `@Size` ignores a null and there is no `@NotNull` beside it, so an
    // omitted list is legal and creates a rule set with no coefficients.
    let absent = app
        .send_as(
            "POST",
            &url,
            admin_id,
            Some(&json!({ "name": "No Weights" })),
        )
        .await;
    assert_eq!(absent.status, 201, "{}", absent.text());
    assert_eq!(absent.json()["coefficients"], json!([]));
}

/// The Admin API's actual security boundary, independent of the UI's two
/// layers of gating.
#[tokio::test]
async fn a_non_admin_manager_is_refused_and_an_anonymous_request_is_challenged() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let url = format!("/api/admin/tournaments/{winter}/scoring-rule-sets");

    let non_admin = app.manager("SherlockMain").await;
    assert!(!non_admin.is_admin);
    assert_eq!(app.get_as(&url, non_admin.id).await.status, 403);
    assert_eq!(app.get(&url).await.status, 401);
}
