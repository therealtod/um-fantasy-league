//! The Roster Builder walkthrough, end to end against the database.
//!
//! It drives the service functions rather than the router; the wire shape
//! is `tournament_api.rs`'s subject.
//!
//! Heroes and tournaments are looked up by name: there is no slug any more, and
//! the seed keys every reference row on its natural name for exactly this
//! reason.

use umfl_domain::DomainError;
use umfl_domain::tournament::EntryStatus;
use umfl_server::error::ApiError;
use umfl_server::hero::{HeroFilter, HeroSort, query as hero_query};
use umfl_server::manager::Manager;
use umfl_server::standings::query as standings_query;
use umfl_server::tournament::{admin_service as tournament_admin_service, query, service};

use crate::harness::TestApp;

/// The rule codes a 422 carried, in order.
///
/// The service raises `ApiError::Domain(DomainError::RosterRule(_))`, whose
/// violations have already been flattened to strings at the policy boundary,
/// so a test that wants the codes reads them back the same way the frontend
/// does.
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
        "update tournaments set capacity = $1 where id = $2",
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
        "insert into tournament_entries (tournament_id, manager_id, status, credit_grant)
         select $1, $2, 'DRAFT', credit_grant from tournaments where id = $1",
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

/// The "no cost snapshot" invariant, exercised: `entry_slots` stores only the
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
        "update tournament_heroes set cost = 3000 where tournament_id = $1 and hero_id = $2",
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
/// `tournament_heroes`. The slot survives, priced at 0, rather than being
/// silently dropped -- which is why `find_roster_heroes` exists alongside
/// `find_by_ids`.
///
/// This closes with a cross-check against `standings::query::rosters`: both
/// reads must report the same roster length and the same spend. That half is
/// not decoration. The two paths reach cost by genuinely
/// different SQL -- the roster read joins `tournament_heroes` through
/// `find_roster_heroes`, while `standings::query::rosters` **left** joins it and
/// leans on `unwrap_or(0)` for the missing row -- so a hero pulled from the pool
/// is the one input that can make them disagree, and a leaderboard that prices a
/// locked roster differently from the Roster Builder is exactly the silent
/// wrongness this suite exists to catch.
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
        "delete from tournament_heroes where tournament_id = $1 and hero_id = $2",
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

    // The cross-check. `spent()` is derived from the slots on this side too,
    // so agreement here means the two queries agree about the *pulled* hero
    // costing 0 rather than vanishing -- a dropped slot would read as a
    // shorter roster and a smaller spend, and would quietly move the board.
    let mut conn = app.pool().acquire().await.unwrap();
    let board_rosters = standings_query::rosters(&mut conn, winter).await.unwrap();
    let mine = board_rosters
        .iter()
        .find(|r| r.manager_id == manager.id)
        .expect("the locked entry is on the board");
    assert_eq!(
        mine.heroes.len(),
        reloaded.heroes.len(),
        "the leaderboard read dropped a slot the roster read kept"
    );
    assert_eq!(
        mine.heroes.iter().map(|h| h.cost).collect::<Vec<_>>(),
        [4_100, 3_200, 0],
        "the pulled hero is priced at 0 on the board, not omitted"
    );
    assert_eq!(mine.spent(), reloaded.budget.spent);
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

// ---------------------------------------------------------------------------
// Between-round swaps.
//
// Winter of Champions is the fixture for these: `db/seed/V5__demo_swap_config
// .sql` gives it one swap per round, and it starts in round 1 with the window
// shut, which is exactly the state an admin drives forward from.
//
// Every test locks a roster costing 8_200 of the 10_000 grant (3400 + 2900 +
// 1900), leaving 1_800 of headroom -- enough to trade up a little, not enough
// to reach Medusa at 5_600. That gap is what makes the budget case a swap the
// manager can plausibly *want* rather than an obvious over-reach.
// ---------------------------------------------------------------------------

/// Register, draft and lock, then leave the tournament in `round`, with the
/// window open or shut. Returns the locked roster's hero ids in slot order.
async fn locked_at_round(
    app: &TestApp,
    winter: i64,
    manager: &Manager,
    round: i32,
    window_open: bool,
) -> Vec<i64> {
    service::register(&app.state, winter, manager)
        .await
        .unwrap();
    let picks = app
        .hero_ids(&["Sherlock Holmes", "Yennenga", "Sinbad"])
        .await;
    service::set_slots(&app.state, winter, manager, &picks)
        .await
        .unwrap();
    service::lock_roster(&app.state, winter, manager)
        .await
        .unwrap();

    for _ in 1..round {
        tournament_admin_service::advance_round(&app.state, winter)
            .await
            .unwrap();
    }
    if window_open {
        tournament_admin_service::set_swap_window(&app.state, winter, true)
            .await
            .unwrap();
    }
    picks
}

async fn swap_log(app: &TestApp, winter: i64, manager_id: i64) -> Vec<(i32, i64, i64)> {
    sqlx::query!(
        "select s.round, s.hero_out_id, s.hero_in_id
           from roster_swaps s
           join tournament_entries e on e.id = s.entry_id
          where e.tournament_id = $1 and e.manager_id = $2
          order by s.round, s.id",
        winter,
        manager_id
    )
    .fetch_all(app.pool())
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.round, r.hero_out_id, r.hero_in_id))
    .collect()
}

#[tokio::test]
async fn a_swap_is_refused_while_the_window_is_shut() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    // Round 2 with the window shut: the allowance is there to spend, so the
    // only thing standing in the way is the window itself.
    let picks = locked_at_round(&app, winter, &manager, 2, false).await;

    let beowulf = app.hero_id("Beowulf").await;
    let proposed = vec![picks[0], picks[1], beowulf];

    let err = service::swap_roster(&app.state, winter, &manager, &proposed)
        .await
        .expect_err("the window is shut");

    assert_eq!(rules(&err), ["SWAP_WINDOW_CLOSED"]);
}

#[tokio::test]
async fn a_swap_is_refused_before_the_roster_is_locked() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    service::register(&app.state, winter, &manager)
        .await
        .unwrap();
    let picks = app
        .hero_ids(&["Sherlock Holmes", "Yennenga", "Sinbad"])
        .await;
    service::set_slots(&app.state, winter, &manager, &picks)
        .await
        .unwrap();
    tournament_admin_service::advance_round(&app.state, winter)
        .await
        .unwrap();
    tournament_admin_service::set_swap_window(&app.state, winter, true)
        .await
        .unwrap();

    let beowulf = app.hero_id("Beowulf").await;
    let err = service::swap_roster(&app.state, winter, &manager, &[picks[0], picks[1], beowulf])
        .await
        .expect_err("an unlocked entry belongs on the draft path");

    assert_eq!(rules(&err), ["SWAP_WINDOW_CLOSED"]);
}

#[tokio::test]
async fn round_one_offers_no_allowance_at_all() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    // Window open, but still in round 1: a window follows a round, and there
    // has not been one yet.
    let picks = locked_at_round(&app, winter, &manager, 1, true).await;

    let beowulf = app.hero_id("Beowulf").await;
    let err = service::swap_roster(&app.state, winter, &manager, &[picks[0], picks[1], beowulf])
        .await
        .expect_err("nothing has happened to react to");

    assert_eq!(rules(&err), ["SWAP_LIMIT_EXCEEDED"]);
}

#[tokio::test]
async fn swapping_more_heroes_than_the_allowance_permits_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;

    let (beowulf, bigfoot) = (app.hero_id("Beowulf").await, app.hero_id("Bigfoot").await);
    let err = service::swap_roster(&app.state, winter, &manager, &[picks[0], beowulf, bigfoot])
        .await
        .expect_err("two heroes on a one-swap allowance");

    assert_eq!(rules(&err), ["SWAP_LIMIT_EXCEEDED"]);
    assert!(
        swap_log(&app, winter, manager.id).await.is_empty(),
        "a rejected swap writes nothing"
    );
}

#[tokio::test]
async fn a_swap_that_breaks_the_budget_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;

    // Sinbad (1900) out, Medusa (5600) in: 3400 + 2900 + 5600 = 11_900, which
    // is 1_900 past the grant.
    let medusa = app.hero_id("Medusa").await;
    let err = service::swap_roster(&app.state, winter, &manager, &[picks[0], picks[1], medusa])
        .await
        .expect_err("11_900 does not fit in 10_000");

    assert_eq!(rules(&err), ["BUDGET_EXCEEDED"]);
}

#[tokio::test]
async fn a_successful_swap_rewrites_the_slots_and_records_the_exchange() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;
    let (sinbad, beowulf) = (picks[2], app.hero_id("Beowulf").await);

    let snapshot =
        service::swap_roster(&app.state, winter, &manager, &[picks[0], picks[1], beowulf])
            .await
            .expect("one hero, one swap, within budget");

    assert_eq!(snapshot.entry.hero_ids(), [picks[0], picks[1], beowulf]);
    assert_eq!(
        snapshot.budget.spent, 8_700,
        "3400 + 2900 + 2400 at Winter prices"
    );
    assert_eq!(
        snapshot.entry.status,
        EntryStatus::Locked,
        "a swap does not unlock"
    );
    assert_eq!(snapshot.swaps_available, 0, "the round's one swap is spent");
    assert!(snapshot.already_swapped_this_round);
    assert!(!snapshot.swappable);

    assert_eq!(
        swap_log(&app, winter, manager.id).await,
        [(2, sinbad, beowulf)],
        "the log names who left, who arrived, and in which round"
    );

    let mut conn = app.pool().acquire().await.unwrap();
    let reloaded = query::find_entry(&mut conn, winter, manager.id)
        .await
        .unwrap()
        .expect("the entry survives");
    assert_eq!(reloaded.hero_ids(), [picks[0], picks[1], beowulf]);
}

#[tokio::test]
async fn a_second_submission_in_the_same_round_is_rejected() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 3, true).await;
    let (beowulf, bigfoot) = (app.hero_id("Beowulf").await, app.hero_id("Bigfoot").await);

    // Round 3 with one swap per round banks an allowance of two, so the second
    // submission is refused by the one-submission rule rather than by running
    // out of swaps -- which is the distinction being pinned here.
    service::swap_roster(&app.state, winter, &manager, &[picks[0], picks[1], beowulf])
        .await
        .expect("the first submission lands");

    let err = service::swap_roster(&app.state, winter, &manager, &[picks[0], bigfoot, beowulf])
        .await
        .expect_err("one submission per round");

    assert_eq!(rules(&err), ["ALREADY_SWAPPED_THIS_ROUND"]);
    assert_eq!(
        swap_log(&app, winter, manager.id).await.len(),
        1,
        "the refused second submission left the first one alone"
    );
}

/// A submission must decide the one-per-round rule *after* it has the entry,
/// not before.
///
/// Nothing indexes "one submission per round" and nothing can: a submission
/// legitimately writes several rows sharing a round, so `(entry_id, round)` is
/// not unique. `swap_roster` therefore takes a row lock on the entry before it
/// reads the log it is about to write to.
///
/// Left unserialised this is not merely a double-spent allowance. The log stops
/// replaying to the roster, and `EntryRoster::holdings` rewinds the duplicate
/// pair into a phantom second holding -- the arriving hero then scores twice on
/// the board for one exchange, and nothing surfaces it until somebody doubts
/// the standings.
///
/// The other submission is played by this test's own transaction rather than a
/// second `swap_roster`: two real calls interleave at whatever points the
/// runtime chooses, and would pass with the lock or without it. Holding the row
/// and *then* writing the submission the queued call has to see is the same
/// race with the timing decided here -- without the lock the call reads the log
/// before this transaction commits and lands a second exchange.
#[tokio::test]
async fn a_second_submission_cannot_slip_past_on_a_read_taken_before_the_lock() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;
    let (beowulf, bigfoot) = (app.hero_id("Beowulf").await, app.hero_id("Bigfoot").await);

    // Stand where the winning submission stands: holding the entry row, with
    // its own exchange not yet committed.
    let mut winner = app.pool().begin().await.expect("begin");
    let entry_id = sqlx::query_scalar!(
        "select id from tournament_entries
          where tournament_id = $1 and manager_id = $2
          for update",
        winter,
        manager.id
    )
    .fetch_one(&mut *winner)
    .await
    .expect("hold the entry row");

    let (state, mine) = (app.state.clone(), manager.clone());
    let proposed = vec![picks[0], picks[1], beowulf];
    let queued =
        tokio::spawn(async move { service::swap_roster(&state, winter, &mine, &proposed).await });

    // Two local statements stand between that call's `begin` and its lock, so
    // this is three orders of magnitude more than it needs to be waiting there.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    sqlx::query!(
        "insert into roster_swaps (entry_id, round, hero_out_id, hero_in_id)
         values ($1, 2, $2, $3)",
        entry_id,
        picks[2],
        bigfoot
    )
    .execute(&mut *winner)
    .await
    .expect("record the winning exchange");
    // The roster moves with the log, the way `swap_roster` writes the two
    // together -- a log that did not agree with the slots is the corruption
    // this test exists to keep out, not a fixture to build it from.
    sqlx::query!(
        "update entry_slots set hero_id = $3 where entry_id = $1 and hero_id = $2",
        entry_id,
        picks[2],
        bigfoot
    )
    .execute(&mut *winner)
    .await
    .expect("move the slot");
    winner.commit().await.expect("commit");

    let err = queued
        .await
        .expect("the queued submission ran")
        .expect_err("the round was spent while it waited");
    assert_eq!(
        rules(&err),
        ["ALREADY_SWAPPED_THIS_ROUND", "SWAP_LIMIT_EXCEEDED"],
        "both gates the winner shut: the round's submission and its allowance"
    );

    assert_eq!(
        swap_log(&app, winter, manager.id).await,
        [(2, picks[2], bigfoot)],
        "one exchange, one row -- not the duplicate pair that scores a hero twice"
    );
}

#[tokio::test]
async fn an_unused_window_is_banked_and_spent_later() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    // Two windows offered (rounds 2 and 3), neither spent.
    let picks = locked_at_round(&app, winter, &manager, 3, true).await;
    let (beowulf, bigfoot) = (app.hero_id("Beowulf").await, app.hero_id("Bigfoot").await);

    let snapshot =
        service::swap_roster(&app.state, winter, &manager, &[picks[0], beowulf, bigfoot])
            .await
            .expect("both banked swaps spent in one submission");

    assert_eq!(snapshot.swaps_available, 0);
    assert_eq!(
        swap_log(&app, winter, manager.id).await.len(),
        2,
        "one row per hero exchanged, both stamped with the round they were spent in"
    );
    assert!(
        swap_log(&app, winter, manager.id)
            .await
            .iter()
            .all(|(round, _, _)| *round == 3)
    );
}

#[tokio::test]
async fn submitting_an_unchanged_roster_spends_nothing() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;

    let snapshot = service::swap_roster(&app.state, winter, &manager, &picks)
        .await
        .expect("changing nothing breaks no rule");

    assert_eq!(snapshot.entry.hero_ids(), picks);
    assert_eq!(
        snapshot.swaps_available, 1,
        "an exchange of nothing costs nothing"
    );
    assert!(
        swap_log(&app, winter, manager.id).await.is_empty(),
        "and records nothing, so the window is still there to use"
    );
}

#[tokio::test]
async fn a_swap_still_cannot_reach_a_hero_outside_the_pool() {
    let app = TestApp::spawn().await;
    let winter = app.tournament_id("Winter of Champions").await;
    let manager = app.manager("SherlockMain").await;
    let picks = locked_at_round(&app, winter, &manager, 2, true).await;

    // Resolved against the pool by `resolve_picks`, exactly as a draft is --
    // the swap path gains no back door into the wider hero catalogue.
    let outsider = app.hero_id("Nikola Tesla").await;
    let err = service::swap_roster(
        &app.state,
        winter,
        &manager,
        &[picks[0], picks[1], outsider],
    )
    .await
    .expect_err("Nikola Tesla is not in Winter's pool");

    assert_eq!(rules(&err), ["UNKNOWN_HERO"]);
}
