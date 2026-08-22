//! The Roster Builder walkthrough, end to end against the database.
//!
//! Oracle: `tournament/RosterFlowIntegrationTest.kt`, ported near 1:1. It
//! drives the service functions rather than the router, exactly as the Kotlin
//! drove `TournamentService` rather than `TournamentController` -- the wire
//! shape is `tournament_api.rs`'s subject.
//!
//! Heroes and tournaments are looked up by name: there is no slug any more, and
//! the seed keys every reference row on its natural name for exactly this
//! reason.

use umfl_domain::DomainError;
use umfl_domain::tournament::EntryStatus;
use umfl_server::error::ApiError;
use umfl_server::hero::{HeroFilter, HeroSort, query as hero_query};
use umfl_server::tournament::{query, service};

use crate::harness::TestApp;

/// The rule codes a 422 carried, in order.
///
/// The service raises `ApiError::Domain(DomainError::RosterRule(_))`, whose
/// violations have already been flattened to strings at the policy boundary --
/// the boundary PORTING.md §5 puts there -- so a test that wants the codes
/// reads them back the same way the frontend does.
fn rules(err: &ApiError) -> Vec<String> {
    match err {
        ApiError::Domain(e @ DomainError::RosterRule(_)) => {
            e.violations().iter().map(|v| v.rule.clone()).collect()
        }
        other => panic!("expected a roster-rule 422, got {other:?}"),
    }
}

fn conflict_message(err: &ApiError) -> String {
    match err {
        ApiError::Domain(e @ DomainError::Conflict(_)) => e.to_string(),
        other => panic!("expected a conflict, got {other:?}"),
    }
}

#[tokio::test]
async fn registering_is_free_and_opens_an_empty_draft_holding_the_grant() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;

    let snapshot = service::register(&app.state, winter, &manager)
        .await
        .expect("registration succeeds");

    assert_eq!(snapshot.entry.status, EntryStatus::Draft);
    assert!(snapshot.entry.slots.is_empty());
    assert_eq!(
        snapshot.entry.credit_grant,
        snapshot.tournament.credit_grant
    );
    assert_eq!(snapshot.budget.spent, 0);
    assert_eq!(snapshot.budget.remaining, 10_000);
}

#[tokio::test]
async fn registering_twice_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;

    service::register(&app.state, winter, &manager)
        .await
        .expect("the first registration succeeds");
    let err = service::register(&app.state, winter, &manager)
        .await
        .expect_err("the second is a conflict");

    assert_eq!(
        conflict_message(&err),
        "Already registered for Winter of Champions."
    );
}

#[tokio::test]
async fn a_full_tournament_is_closed_to_new_entries() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    service::register(&app.state, winter, &app.manager("SherlockMain").await)
        .await
        .expect("the first seat is free");

    // Shrink capacity to what is already taken rather than registering 64
    // managers the seed does not have.
    let taken = query::count_entries(app.pool(), winter).await.unwrap();
    sqlx::query!(
        "update tournament set capacity = $1 where id = $2",
        i32::try_from(taken).unwrap(),
        winter
    )
    .execute(app.pool())
    .await
    .unwrap();

    let err = service::register(&app.state, winter, &app.manager("MythicMind").await)
        .await
        .expect_err("the tournament is full");
    let message = conflict_message(&err);
    assert!(message.contains("is full"), "{message}");
}

#[tokio::test]
async fn a_finished_tournament_is_closed_to_new_entries() {
    let app = TestApp::spawn().await;
    let summer = app.tournament_id("Summer of Legends").await;

    let err = service::register(&app.state, summer, &app.manager("SherlockMain").await)
        .await
        .expect_err("a completed tournament takes no entries");
    let message = conflict_message(&err);
    assert!(message.contains("COMPLETED"), "{message}");
}

#[tokio::test]
async fn an_over_budget_draft_is_saved_but_cannot_be_locked() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("ArthurianLegend").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    // Sun Wukong 5300 + Medusa 5600 + King Arthur 4700 = 15,600 against a
    // 10,000 grant.
    let premium = app.hero_ids(&["Sun Wukong", "Medusa", "King Arthur"]).await;
    let over_budget = service::set_slots(&app.state, winter, &manager, &premium)
        .await
        .expect("a draft may go over budget");

    assert_eq!(over_budget.budget.spent, 15_600);
    assert_eq!(over_budget.budget.remaining, -5_600);
    assert_eq!(over_budget.entry.slots.len(), 3);

    let err = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect_err("locking enforces the budget");
    assert_eq!(rules(&err), ["BUDGET_EXCEEDED"]);

    // Swapping to a legal trio unblocks the lock.
    let legal = app.hero_ids(&["Alice", "Robin Hood", "Bigfoot"]).await;
    let fixed = service::set_slots(&app.state, winter, &manager, &legal)
        .await
        .unwrap();
    assert_eq!(fixed.budget.spent, 9_400);
    assert_eq!(fixed.budget.remaining, 600);

    let locked = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect("a legal roster locks");
    assert_eq!(locked.entry.status, EntryStatus::Locked);
    assert!(locked.entry.locked_at.is_some());
}

#[tokio::test]
async fn the_lobbys_entry_status_projection_tracks_registration_and_locking() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;

    // Every seeded manager holds a locked Summer of Legends entry; Winter
    // starts empty.
    let before = query::statuses_by_tournament(app.pool(), manager.id)
        .await
        .unwrap();
    assert_eq!(before.get(&winter), None);

    service::register(&app.state, winter, &manager)
        .await
        .unwrap();
    let after_register = query::statuses_by_tournament(app.pool(), manager.id)
        .await
        .unwrap();
    assert_eq!(after_register.get(&winter), Some(&EntryStatus::Draft));

    let picks = app.hero_ids(&["Alice", "Robin Hood", "Bigfoot"]).await;
    service::set_slots(&app.state, winter, &manager, &picks)
        .await
        .unwrap();
    service::lock_roster(&app.state, winter, &manager)
        .await
        .unwrap();

    let statuses = query::statuses_by_tournament(app.pool(), manager.id)
        .await
        .unwrap();
    assert_eq!(statuses.get(&winter), Some(&EntryStatus::Locked));
    assert!(
        statuses.len() > 1,
        "the seeded Summer of Legends entry is still in the projection"
    );
}

#[tokio::test]
async fn a_locked_roster_is_immutable_and_survives_a_reload() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("NeonStrategist").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    let picks = app
        .hero_ids(&["Sherlock Holmes", "Yennenga", "Sinbad"])
        .await;
    service::set_slots(&app.state, winter, &manager, &picks)
        .await
        .unwrap();
    let locked = service::lock_roster(&app.state, winter, &manager)
        .await
        .unwrap();
    assert_eq!(
        locked.budget.spent, 8_200,
        "3400 + 2900 + 1900 at Winter prices"
    );

    let mut reversed = picks.clone();
    reversed.reverse();
    let err = service::set_slots(&app.state, winter, &manager, &reversed)
        .await
        .expect_err("a locked roster is immutable");
    assert_eq!(rules(&err), ["ENTRY_LOCKED"]);

    let mut conn = app.pool().acquire().await.unwrap();
    let reloaded = query::find_entry(&mut conn, winter, manager.id)
        .await
        .unwrap()
        .expect("the entry is still there");
    assert_eq!(reloaded.status, EntryStatus::Locked);
    assert_eq!(
        reloaded.hero_ids(),
        picks,
        "slot order must survive the round trip"
    );
}

#[tokio::test]
async fn duplicate_picks_are_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    let alice = app.hero_id("Alice").await;
    let err = service::set_slots(&app.state, winter, &manager, &[alice, alice])
        .await
        .expect_err("a hero may only be selected once");
    assert_eq!(rules(&err), ["DUPLICATE_HERO"]);
}

#[tokio::test]
async fn a_hero_id_that_does_not_exist_at_all_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    let err = service::set_slots(&app.state, winter, &manager, &[999_999])
        .await
        .expect_err("no such hero");
    assert_eq!(rules(&err), ["UNKNOWN_HERO"]);
}

/// Spring of Myths carries eight of the twelve heroes; Bigfoot is not one of
/// them, so this is `UNKNOWN_HERO` with an id that genuinely exists -- the check
/// is "in this tournament's pool", not "exists".
#[tokio::test]
async fn a_real_hero_outside_this_tournaments_pool_is_just_as_unknown() {
    let app = TestApp::spawn().await;
    let spring = app.tournament_id("Spring of Myths").await;
    let manager = app.manager("NeonStrategist").await;

    // Spring is SCHEDULED, so it takes no registrations yet -- the entry is
    // created directly. Rosters still accept changes in that state.
    sqlx::query!(
        "insert into tournament_entry (tournament_id, manager_id, status, credit_grant)
         select $1, $2, 'DRAFT', credit_grant from tournament where id = $1",
        spring,
        manager.id
    )
    .execute(app.pool())
    .await
    .unwrap();

    let bigfoot = app.hero_id("Bigfoot").await;
    assert!(
        hero_query::find_by_ids(app.pool(), spring, &[bigfoot])
            .await
            .unwrap()
            .is_empty(),
        "precondition: Bigfoot is outside Spring's pool"
    );

    let err = service::set_slots(&app.state, spring, &manager, &[bigfoot])
        .await
        .expect_err("Bigfoot is not in Spring's pool");
    assert_eq!(rules(&err), ["UNKNOWN_HERO"]);
    let ApiError::Domain(domain) = &err else {
        unreachable!()
    };
    let message = domain.to_string();
    assert!(message.contains("Spring of Myths"), "{message}");
}

/// The "no cost snapshot" invariant, exercised: `entry_slot` stores only the
/// hero, so re-pricing the pool re-prices an unlocked roster.
#[tokio::test]
async fn re_pricing_a_hero_re_prices_an_unlocked_roster() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("MythicMind").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    let picks = app.hero_ids(&["Alice", "Robin Hood", "Bigfoot"]).await;
    let drafted = service::set_slots(&app.state, winter, &manager, &picks)
        .await
        .unwrap();
    assert_eq!(drafted.budget.spent, 9_400);

    // An admin retunes Bigfoot from 2,100 to 3,000. Nothing was snapshotted, so
    // the draft re-prices itself and is now over budget.
    let bigfoot = app.hero_id("Bigfoot").await;
    sqlx::query!(
        "update tournament_hero set cost = 3000 where tournament_id = $1 and hero_id = $2",
        winter,
        bigfoot
    )
    .execute(app.pool())
    .await
    .unwrap();

    let reloaded = service::find_my_entry(&app.state, winter, &manager)
        .await
        .unwrap()
        .expect("the entry is still there");
    assert_eq!(reloaded.budget.spent, 10_300);
    assert_eq!(reloaded.budget.remaining, -300);
    assert_eq!(
        reloaded.heroes.iter().map(|h| h.cost).collect::<Vec<_>>(),
        [4_100, 3_200, 3_000]
    );

    let err = service::lock_roster(&app.state, winter, &manager)
        .await
        .expect_err("the re-priced roster is over budget");
    assert_eq!(rules(&err), ["BUDGET_EXCEEDED"]);
}

/// UMFL-06: a hero still on a locked roster is later pulled from
/// `tournament_hero`. The slot survives, priced at 0, rather than being
/// silently dropped -- which is why `find_roster_heroes` exists alongside
/// `find_by_ids`.
///
/// The Kotlin closes with a cross-check against `StandingsQuery.rosters`: that
/// both reads report the same roster length and spend. That half waits for the
/// standings feature; the invariant this test owns is asserted in full here.
#[tokio::test]
async fn a_hero_pulled_from_the_pool_after_locking_is_kept_at_cost_zero() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();

    let picks = app.hero_ids(&["Alice", "Robin Hood", "Bigfoot"]).await;
    service::set_slots(&app.state, winter, &manager, &picks)
        .await
        .unwrap();
    service::lock_roster(&app.state, winter, &manager)
        .await
        .unwrap();

    let bigfoot = app.hero_id("Bigfoot").await;
    sqlx::query!(
        "delete from tournament_hero where tournament_id = $1 and hero_id = $2",
        winter,
        bigfoot
    )
    .execute(app.pool())
    .await
    .unwrap();

    let reloaded = service::find_my_entry(&app.state, winter, &manager)
        .await
        .unwrap()
        .expect("the entry is still there");
    assert_eq!(
        reloaded.heroes.len(),
        3,
        "the slot survives, priced at 0, not silently dropped"
    );
    assert_eq!(
        reloaded.heroes.iter().map(|h| h.cost).collect::<Vec<_>>(),
        [4_100, 3_200, 0]
    );
    assert_eq!(reloaded.budget.spent, 7_300);
}

#[tokio::test]
async fn the_hero_pool_is_scoped_to_the_tournament_priced_and_sortable() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;

    let by_cost = hero_query::find_by_tournament(app.pool(), winter, &HeroFilter::default())
        .await
        .unwrap();
    assert_eq!(by_cost.len(), 12);
    assert_eq!(by_cost[0].name, "Medusa", "the most expensive Winter pick");
    assert_eq!(by_cost[0].cost, 5_600);
    let costs: Vec<i32> = by_cost.iter().map(|h| h.cost).collect();
    let mut descending = costs.clone();
    descending.sort_by(|a, b| b.cmp(a));
    assert_eq!(costs, descending);

    let by_name = hero_query::find_by_tournament(
        app.pool(),
        winter,
        &HeroFilter {
            search: None,
            sort: HeroSort::Name,
        },
    )
    .await
    .unwrap();
    let names: Vec<&str> = by_name.iter().map(|h| h.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "NAME sorts alphabetically, ignoring cost");

    assert_eq!(
        search(&app, winter, "ho").await,
        ["Robin Hood", "Sherlock Holmes"]
    );
    assert!(
        search(&app, winter, "_").await.is_empty(),
        "_ is a literal, not a match-any-char wildcard"
    );
    assert!(
        search(&app, winter, "%").await.is_empty(),
        "% is a literal, not a match-all wildcard"
    );
}

async fn search(app: &TestApp, tournament_id: i64, term: &str) -> Vec<String> {
    let mut names: Vec<String> = hero_query::find_by_tournament(
        app.pool(),
        tournament_id,
        &HeroFilter {
            search: Some(term.to_owned()),
            sort: HeroSort::Cost,
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|h| h.name)
    .collect();
    names.sort();
    names
}
