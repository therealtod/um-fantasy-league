//! The league's roster rules, as pure functions.
//!
//! `frontend/src/domain/rosterPolicy.ts` deliberately mirrors [`budget_status`]
//! so the Roster Builder's meter reacts on click, but this side is
//! authoritative. **If you change the arithmetic here, change it there too** --
//! AGENTS.md names that pair explicitly.

use crate::Violation;
use crate::tournament::{EntryStatus, Tournament, TournamentEntry};
use indexmap::IndexMap;

/// The roster rule vocabulary, one constant per way a roster can be wrong.
///
/// The `rule` field the frontend reads off `ApiError.violations` is the constant
/// name, so [`RosterRule::as_str`] is wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RosterRule {
    /// The entry is locked; its roster can no longer change.
    EntryLocked,

    /// The tournament has started or finished; rosters are frozen.
    TournamentClosed,

    /// More heroes selected than the tournament's roster size allows.
    TooManyPicks,

    /// Fewer heroes than the roster size -- cannot lock a partial roster.
    IncompleteRoster,

    /// The same hero appears more than once.
    DuplicateHero,

    /// A selected hero id is not in this tournament's hero pool.
    UnknownHero,

    /// Combined cost is over the entry's credit grant.
    BudgetExceeded,

    /// No swap window is open on this tournament, so a locked roster stays
    /// locked. Also covers a COMPLETED tournament, whose results are final
    /// whatever the window flag says.
    SwapWindowClosed,

    /// More heroes exchanged than the entry's remaining allowance permits.
    SwapLimitExceeded,

    /// This entry already submitted its swaps for the current round. A window
    /// grants one submission, not a running budget to spend a click at a time.
    AlreadySwappedThisRound,
}

impl RosterRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EntryLocked => "ENTRY_LOCKED",
            Self::TournamentClosed => "TOURNAMENT_CLOSED",
            Self::TooManyPicks => "TOO_MANY_PICKS",
            Self::IncompleteRoster => "INCOMPLETE_ROSTER",
            Self::DuplicateHero => "DUPLICATE_HERO",
            Self::UnknownHero => "UNKNOWN_HERO",
            Self::BudgetExceeded => "BUDGET_EXCEEDED",
            Self::SwapWindowClosed => "SWAP_WINDOW_CLOSED",
            Self::SwapLimitExceeded => "SWAP_LIMIT_EXCEEDED",
            Self::AlreadySwappedThisRound => "ALREADY_SWAPPED_THIS_ROUND",
        }
    }
}

impl std::fmt::Display for RosterRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One broken roster rule.
///
/// Kept as its own type rather than [`Violation`] so tests can compare rules
/// without string matching; it converts at the service boundary, which is what
/// lets this module define its vocabulary without touching `error.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterViolation {
    pub rule: RosterRule,
    pub message: String,
}

impl RosterViolation {
    fn new(rule: RosterRule, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
        }
    }
}

impl From<RosterViolation> for Violation {
    fn from(v: RosterViolation) -> Self {
        Violation::new(v.rule.as_str(), v.message)
    }
}

/// Where a roster's spend sits against its budget -- drives the Roster Builder
/// meter.
///
/// `remaining` goes negative when the roster is over budget, and `utilisation`
/// past 1.0; there are no bands, because "how full is the bar" is the whole
/// question the meter answers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BudgetStatus {
    pub spent: i32,
    pub credit_grant: i32,
    pub remaining: i32,
    pub utilisation: f64,
}

/// A prospective roster pick: which hero, and what it costs in this tournament.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterPick {
    pub hero_id: i64,
    pub cost: i32,
}

/// # Panics
///
/// On a non-positive `credit_grant` -- there is no dedicated error variant for
/// it, so it reaches `CatchPanicLayer` (see `http::panic_response`) as a 500.
pub fn budget_status(picks: &[RosterPick], credit_grant: i32) -> BudgetStatus {
    assert!(credit_grant > 0, "Credit grant must be positive");
    let spent: i32 = picks.iter().map(|p| p.cost).sum();
    BudgetStatus {
        spent,
        credit_grant,
        remaining: credit_grant - spent,
        utilisation: f64::from(spent) / f64::from(credit_grant),
    }
}

/// Rules that apply while a roster is still being drafted.
///
/// Going over budget is deliberately *not* a violation here: a draft is a
/// scratchpad, and the Roster Builder shows a negative meter rather than
/// refusing the edit. The budget is enforced at [`validate_lock`].
///
/// Every broken rule is reported, not just the first, so the UI can highlight
/// everything wrong in one pass.
pub fn validate_draft(
    picks: &[RosterPick],
    tournament: &Tournament,
    entry_status: EntryStatus,
) -> Vec<RosterViolation> {
    let mut violations = mutability_violations(tournament, entry_status);
    violations.extend(too_many_picks(picks, tournament));
    violations.extend(duplicate_heroes(picks));
    violations
}

/// More heroes than the roster holds. Shared by all three paths -- a draft, a
/// lock and a swap are equally incapable of seating a fourth hero.
fn too_many_picks(picks: &[RosterPick], tournament: &Tournament) -> Option<RosterViolation> {
    let selected = i32::try_from(picks.len()).unwrap_or(i32::MAX);
    (selected > tournament.roster_size).then(|| {
        RosterViolation::new(
            RosterRule::TooManyPicks,
            format!(
                "Selected {selected} heroes but the roster size is {}.",
                tournament.roster_size
            ),
        )
    })
}

/// Fewer heroes than the roster holds.
///
/// Deliberately *not* part of [`validate_draft`]: a half-filled draft is a
/// scratchpad, not an error. It bites at commit time -- locking or swapping.
fn incomplete_roster(picks: &[RosterPick], tournament: &Tournament) -> Option<RosterViolation> {
    let selected = i32::try_from(picks.len()).unwrap_or(i32::MAX);
    (selected < tournament.roster_size).then(|| {
        RosterViolation::new(
            RosterRule::IncompleteRoster,
            format!(
                "Roster needs {} heroes but only {selected} selected.",
                tournament.roster_size
            ),
        )
    })
}

/// The same hero taken twice.
fn duplicate_heroes(picks: &[RosterPick]) -> Option<RosterViolation> {
    // `IndexMap` keeps encounter order, but the sort below is what actually
    // fixes the message.
    let mut counts: IndexMap<i64, usize> = IndexMap::new();
    for pick in picks {
        *counts.entry(pick.hero_id).or_default() += 1;
    }
    let mut duplicates: Vec<i64> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(id, _)| id)
        .collect();
    duplicates.sort();

    (!duplicates.is_empty()).then(|| {
        let ids = duplicates
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        RosterViolation::new(
            RosterRule::DuplicateHero,
            format!("A hero may only be selected once (repeated ids: {ids})."),
        )
    })
}

/// Everything [`validate_draft`] checks, plus the rules that only bite at commit
/// time.
///
/// The budget comes from the *entry*, not the tournament: the grant was
/// snapshotted at registration and is what this manager actually has to spend.
pub fn validate_lock(
    picks: &[RosterPick],
    tournament: &Tournament,
    entry: &TournamentEntry,
) -> Vec<RosterViolation> {
    let mut violations = validate_draft(picks, tournament, entry.status);
    violations.extend(incomplete_roster(picks, tournament));

    let budget = budget_status(picks, entry.credit_grant);
    if budget.spent > budget.credit_grant {
        violations.push(RosterViolation::new(
            RosterRule::BudgetExceeded,
            format!(
                "Roster costs {} credits, exceeding the {} grant by {}.",
                budget.spent, budget.credit_grant, -budget.remaining
            ),
        ));
    }

    violations
}

/// How many heroes this entry may still exchange.
///
/// Unused allowance carries over, which is the whole reason this is arithmetic
/// over a count rather than a stored counter: a manager who sat out a window
/// banks it by simply never having spent, and nothing has to notice that they
/// did. Windows follow rounds, so round 1 offers nothing -- there is no result
/// yet to react to -- and the Nth round has offered `N - 1` of them.
///
/// Never negative: an allowance that has somehow been overspent (a
/// `swaps_per_round` an admin lowered after the fact) reads as 0 rather than as
/// a debt the manager has to work off.
///
/// `frontend/src/domain/rosterPolicy.ts` mirrors this for the builder's
/// counter. **Change one, change the other.**
pub fn swap_allowance(swaps_per_round: i32, current_round: i32, swaps_used_total: i32) -> i32 {
    let windows_offered = (current_round - 1).max(0);
    (swaps_per_round.saturating_mul(windows_offered) - swaps_used_total).max(0)
}

/// The number of heroes that differ between the roster held and the roster
/// proposed -- i.e. how much of the allowance this submission spends.
///
/// Counted as "heroes arriving", which for a same-size roster is also the
/// number leaving. `validate_swap` reports `IncompleteRoster`/`TooManyPicks`
/// when the sizes differ, so a lopsided count never reaches the limit check as
/// the only complaint.
fn heroes_changed(current_hero_ids: &[i64], proposed: &[RosterPick]) -> i32 {
    let held: std::collections::HashSet<i64> = current_hero_ids.iter().copied().collect();
    let arriving = proposed
        .iter()
        .filter(|p| !held.contains(&p.hero_id))
        .count();
    i32::try_from(arriving).unwrap_or(i32::MAX)
}

/// The rules for exchanging heroes on an already-locked roster, between rounds.
///
/// This is a third path beside [`validate_draft`] and [`validate_lock`], not a
/// variation on either, and the difference that matters is
/// [`Tournament::accepts_roster_changes`]: it is *false* exactly when a swap is
/// legal, because a swap happens while the tournament is LIVE. So this function
/// deliberately never consults it, and gates on
/// [`Tournament::accepts_swaps`] -- the admin's open window -- instead.
///
/// A swap is an exchange rather than a re-draft, so the roster still has to
/// come out at exactly `roster_size`, and it is still priced against the grant
/// snapshotted on the *entry*. What is new is the allowance and the
/// one-submission-per-window rule.
///
/// Every broken rule is reported, not just the first, exactly as in the other
/// two.
pub fn validate_swap(
    current_hero_ids: &[i64],
    proposed: &[RosterPick],
    tournament: &Tournament,
    entry: &TournamentEntry,
    swaps_used_total: i32,
    already_swapped_this_round: bool,
) -> Vec<RosterViolation> {
    let mut violations = Vec::new();

    if !tournament.accepts_swaps() {
        violations.push(RosterViolation::new(
            RosterRule::SwapWindowClosed,
            format!(
                "{} is not accepting roster swaps right now.",
                tournament.name
            ),
        ));
    }

    // An unlocked entry has no roster to swap *from*: it belongs on the
    // ordinary draft-then-lock path, where its picks are still free to change.
    if !entry.is_locked() {
        violations.push(RosterViolation::new(
            RosterRule::SwapWindowClosed,
            "Lock your roster before swapping heroes.",
        ));
    }

    if already_swapped_this_round {
        violations.push(RosterViolation::new(
            RosterRule::AlreadySwappedThisRound,
            format!(
                "You have already used your swap for round {}.",
                tournament.current_round
            ),
        ));
    }

    // A swap is an exchange, not a re-draft: the roster comes out at exactly
    // `roster_size`, so both size rules apply where a draft only gets one.
    violations.extend(too_many_picks(proposed, tournament));
    violations.extend(incomplete_roster(proposed, tournament));
    violations.extend(duplicate_heroes(proposed));

    let requested = heroes_changed(current_hero_ids, proposed);
    let available = swap_allowance(
        tournament.swaps_per_round,
        tournament.current_round,
        swaps_used_total,
    );
    if requested > available {
        violations.push(RosterViolation::new(
            RosterRule::SwapLimitExceeded,
            format!("Swapping {requested} heroes, but you have {available} swaps available."),
        ));
    }

    let budget = budget_status(proposed, entry.credit_grant);
    if budget.spent > budget.credit_grant {
        violations.push(RosterViolation::new(
            RosterRule::BudgetExceeded,
            format!(
                "Roster costs {} credits, exceeding the {} grant by {}.",
                budget.spent, budget.credit_grant, -budget.remaining
            ),
        ));
    }

    violations
}

fn mutability_violations(
    tournament: &Tournament,
    entry_status: EntryStatus,
) -> Vec<RosterViolation> {
    let mut violations = Vec::new();
    if entry_status == EntryStatus::Locked {
        violations.push(RosterViolation::new(
            RosterRule::EntryLocked,
            "This roster is locked and can no longer be changed.",
        ));
    }
    if !tournament.accepts_roster_changes() {
        violations.push(RosterViolation::new(
            RosterRule::TournamentClosed,
            format!(
                "{} is {} and no longer accepts roster changes.",
                tournament.name, tournament.status
            ),
        ));
    }
    violations
}

/// The roster rules, exercised directly.
///
/// Cost literals are the seeded `tournament_heroes` prices for Winter of
/// Champions, so a change to the seed that breaks the "one premium plus two
/// budget picks just fits" tuning shows up here as well as in the integration
/// tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament::{TournamentFormat, TournamentStatus};
    use chrono::NaiveDate;

    fn tournament_with(
        status: TournamentStatus,
        roster_size: i32,
        credit_grant: i32,
    ) -> Tournament {
        Tournament {
            id: Some(2),
            name: "Winter of Champions".into(),
            format: TournamentFormat::Arsenal,
            status,
            start_date: NaiveDate::from_ymd_opt(2026, 8, 14).unwrap(),
            end_date: None,
            capacity: 64,
            roster_size,
            credit_grant,
            current_round: 1,
            swaps_per_round: 0,
            swap_window_open: false,
        }
    }

    fn tournament() -> Tournament {
        tournament_with(TournamentStatus::RegistrationOpen, 3, 10_000)
    }

    /// A LIVE tournament with an open swap window -- the only state in which a
    /// swap is ever legal, so every `swap_validation` test starts here and
    /// breaks one thing at a time.
    fn swap_tournament(current_round: i32, swaps_per_round: i32) -> Tournament {
        Tournament {
            current_round,
            swaps_per_round,
            swap_window_open: true,
            ..tournament_with(TournamentStatus::Live, 3, 10_000)
        }
    }

    fn entry_with(status: EntryStatus, credit_grant: i32) -> TournamentEntry {
        TournamentEntry {
            id: Some(10),
            tournament_id: 2,
            manager_id: 1,
            status,
            credit_grant,
            registered_at: "2026-07-30T09:00:00Z".parse().unwrap(),
            locked_at: match status {
                EntryStatus::Locked => Some("2026-08-01T09:00:00Z".parse().unwrap()),
                EntryStatus::Draft => None,
            },
            slots: Vec::new(),
        }
    }

    fn entry() -> TournamentEntry {
        entry_with(EntryStatus::Draft, 10_000)
    }

    /// Builds a pick list from bare costs: hero ids are 1-based positions, so
    /// every pick is distinct unless a test builds the list by hand.
    fn picks(costs: &[i32]) -> Vec<RosterPick> {
        costs
            .iter()
            .enumerate()
            .map(|(index, &cost)| RosterPick {
                hero_id: index as i64 + 1,
                cost,
            })
            .collect()
    }

    fn rules(violations: &[RosterViolation]) -> Vec<RosterRule> {
        violations.iter().map(|v| v.rule).collect()
    }

    mod budget {
        use super::*;

        #[test]
        fn an_empty_roster_has_spent_nothing() {
            let budget = budget_status(&[], 10_000);

            assert_eq!(budget.spent, 0);
            assert_eq!(budget.credit_grant, 10_000);
            assert_eq!(budget.remaining, 10_000);
            assert_eq!(budget.utilisation, 0.0);
        }

        #[test]
        fn a_partial_roster_reports_what_is_left() {
            let budget = budget_status(&picks(&[4_700]), 10_000);

            assert_eq!(budget.spent, 4_700);
            assert_eq!(budget.remaining, 5_300);
            assert_eq!(budget.utilisation, 0.47);
        }

        #[test]
        fn the_seeded_legal_trio_just_fits_the_grant() {
            // Alice 4100 + Robin Hood 3200 + Bigfoot 2100.
            let budget = budget_status(&picks(&[4_100, 3_200, 2_100]), 10_000);

            assert_eq!(budget.spent, 9_400);
            assert_eq!(budget.remaining, 600);
            assert_eq!(budget.utilisation, 0.94);
        }

        #[test]
        fn the_seeded_premium_trio_busts_it() {
            // Sun Wukong 5300 + Medusa 5600 + King Arthur 4700, at Winter prices.
            let budget = budget_status(&picks(&[5_300, 5_600, 4_700]), 10_000);

            assert_eq!(budget.spent, 15_600);
            assert_eq!(budget.remaining, -5_600);
            assert_eq!(budget.utilisation, 1.56);
        }

        #[test]
        fn spending_the_grant_to_the_last_credit_is_not_over_budget() {
            let budget = budget_status(&picks(&[10_000]), 10_000);

            assert_eq!(budget.remaining, 0);
            assert_eq!(budget.utilisation, 1.0);
        }

        #[test]
        #[should_panic(expected = "Credit grant must be positive")]
        fn a_non_positive_grant_is_a_programming_error() {
            budget_status(&[], 0);
        }
    }

    mod draft_validation {
        use super::*;

        #[test]
        fn a_partial_roster_is_a_perfectly_good_draft() {
            let violations = validate_draft(&picks(&[4_700]), &tournament(), EntryStatus::Draft);

            assert!(violations.is_empty(), "expected none, got {violations:?}");
        }

        #[test]
        fn going_over_budget_is_allowed_while_drafting() {
            // A draft is a scratchpad; the budget bites at lock time.
            let violations = validate_draft(
                &picks(&[5_300, 5_600, 4_700]),
                &tournament(),
                EntryStatus::Draft,
            );

            assert!(violations.is_empty(), "expected none, got {violations:?}");
        }

        #[test]
        fn selecting_more_heroes_than_the_roster_size_is_rejected() {
            let violations = validate_draft(
                &picks(&[1_000, 1_000, 1_000, 1_000]),
                &tournament(),
                EntryStatus::Draft,
            );

            assert_eq!(rules(&violations), vec![RosterRule::TooManyPicks]);
            assert_eq!(
                violations[0].message,
                "Selected 4 heroes but the roster size is 3."
            );
        }

        #[test]
        fn the_same_hero_cannot_be_picked_twice() {
            let duplicated = vec![
                RosterPick {
                    hero_id: 7,
                    cost: 2_100,
                },
                RosterPick {
                    hero_id: 7,
                    cost: 2_100,
                },
            ];

            let violations = validate_draft(&duplicated, &tournament(), EntryStatus::Draft);

            assert_eq!(rules(&violations), vec![RosterRule::DuplicateHero]);
            assert_eq!(
                violations[0].message,
                "A hero may only be selected once (repeated ids: 7)."
            );
        }

        #[test]
        fn a_locked_entry_cannot_be_edited() {
            let violations = validate_draft(&picks(&[1_000]), &tournament(), EntryStatus::Locked);

            assert_eq!(rules(&violations), vec![RosterRule::EntryLocked]);
        }

        #[test]
        fn rosters_freeze_once_the_tournament_is_live() {
            let live = tournament_with(TournamentStatus::Live, 3, 10_000);

            let violations = validate_draft(&picks(&[1_000]), &live, EntryStatus::Draft);

            assert_eq!(rules(&violations), vec![RosterRule::TournamentClosed]);
            assert_eq!(
                violations[0].message,
                "Winter of Champions is LIVE and no longer accepts roster changes."
            );
        }

        #[test]
        fn every_broken_rule_is_reported_not_just_the_first() {
            let duplicated = vec![
                RosterPick {
                    hero_id: 7,
                    cost: 1_000,
                },
                RosterPick {
                    hero_id: 7,
                    cost: 1_000,
                },
                RosterPick {
                    hero_id: 8,
                    cost: 1_000,
                },
                RosterPick {
                    hero_id: 9,
                    cost: 1_000,
                },
            ];

            let violations = validate_draft(
                &duplicated,
                &tournament_with(TournamentStatus::Completed, 3, 10_000),
                EntryStatus::Locked,
            );

            assert_eq!(
                rules(&violations),
                vec![
                    RosterRule::EntryLocked,
                    RosterRule::TournamentClosed,
                    RosterRule::TooManyPicks,
                    RosterRule::DuplicateHero,
                ]
            );
        }
    }

    mod lock_validation {
        use super::*;

        #[test]
        fn a_full_roster_within_budget_locks_cleanly() {
            let violations = validate_lock(&picks(&[4_100, 3_200, 2_100]), &tournament(), &entry());

            assert!(violations.is_empty(), "expected none, got {violations:?}");
        }

        #[test]
        fn a_partial_roster_cannot_be_locked() {
            let violations = validate_lock(&picks(&[4_700, 3_200]), &tournament(), &entry());

            assert_eq!(rules(&violations), vec![RosterRule::IncompleteRoster]);
            assert_eq!(
                violations[0].message,
                "Roster needs 3 heroes but only 2 selected."
            );
        }

        #[test]
        fn an_over_budget_roster_cannot_be_locked() {
            let violations = validate_lock(&picks(&[5_300, 5_600, 4_700]), &tournament(), &entry());

            assert_eq!(rules(&violations), vec![RosterRule::BudgetExceeded]);
            assert_eq!(
                violations[0].message,
                "Roster costs 15600 credits, exceeding the 10000 grant by 5600."
            );
        }

        #[test]
        fn spending_the_grant_to_the_last_credit_is_allowed() {
            let violations = validate_lock(&picks(&[5_000, 3_000, 2_000]), &tournament(), &entry());

            assert!(violations.is_empty(), "expected none, got {violations:?}");
        }

        #[test]
        fn an_already_locked_roster_cannot_be_locked_again() {
            let violations = validate_lock(
                &picks(&[4_100, 3_200, 2_100]),
                &tournament(),
                &entry_with(EntryStatus::Locked, 10_000),
            );

            assert_eq!(rules(&violations), vec![RosterRule::EntryLocked]);
        }

        #[test]
        fn an_incomplete_over_budget_roster_reports_both_problems() {
            let violations = validate_lock(&picks(&[9_000, 9_000]), &tournament(), &entry());

            assert_eq!(
                rules(&violations),
                vec![RosterRule::IncompleteRoster, RosterRule::BudgetExceeded]
            );
        }

        #[test]
        fn the_budget_comes_from_the_entry_not_the_tournament() {
            // The tournament's grant was raised to 20,000 after this manager
            // registered on 10,000 -- their snapshot is what binds.
            let generous_now = tournament_with(TournamentStatus::RegistrationOpen, 3, 20_000);

            let violations = validate_lock(
                &picks(&[6_000, 6_000, 6_000]),
                &generous_now,
                &entry_with(EntryStatus::Draft, 10_000),
            );

            assert_eq!(rules(&violations), vec![RosterRule::BudgetExceeded]);
            assert!(
                violations[0].message.contains("10000"),
                "{}",
                violations[0].message
            );
        }

        #[test]
        fn roster_size_comes_from_the_tournament_not_a_constant() {
            let five_hero_league = tournament_with(TournamentStatus::RegistrationOpen, 5, 10_000);

            assert!(
                validate_lock(
                    &picks(&[2_000, 2_000, 2_000, 2_000, 2_000]),
                    &five_hero_league,
                    &entry(),
                )
                .is_empty()
            );
            assert!(
                !validate_lock(&picks(&[2_000, 2_000, 2_000]), &five_hero_league, &entry())
                    .is_empty()
            );
        }
    }

    mod swap_allowance_arithmetic {
        use super::*;

        /// Windows follow rounds, so the first round offers none: there is no
        /// result yet to react to.
        #[test]
        fn round_one_offers_nothing() {
            assert_eq!(swap_allowance(2, 1, 0), 0);
        }

        #[test]
        fn each_further_round_offers_another_window() {
            assert_eq!(swap_allowance(2, 2, 0), 2);
            assert_eq!(swap_allowance(2, 3, 0), 4);
            assert_eq!(swap_allowance(2, 4, 0), 6);
        }

        /// The point of deriving this from a count rather than storing a
        /// counter: sitting a window out banks it, with nothing having to
        /// notice that it happened.
        #[test]
        fn unused_allowance_carries_over() {
            // One per round, round 3, nothing spent: both windows are still
            // there.
            assert_eq!(swap_allowance(1, 3, 0), 2);
            // One of them spent in round 2 leaves one.
            assert_eq!(swap_allowance(1, 3, 1), 1);
            assert_eq!(swap_allowance(1, 3, 2), 0);
        }

        #[test]
        fn a_tournament_with_no_allowance_never_offers_one() {
            assert_eq!(swap_allowance(0, 9, 0), 0);
        }

        /// An admin who lowers `swaps_per_round` after managers have already
        /// spent must not leave them owing swaps back.
        #[test]
        fn an_overspent_allowance_reads_as_zero_not_a_debt() {
            assert_eq!(swap_allowance(1, 2, 5), 0);
        }
    }

    mod swap_validation {
        use super::*;

        /// The seeded Winter of Champions trio: 4100 + 3200 + 2100 = 9,400.
        fn held() -> Vec<i64> {
            vec![1, 2, 3]
        }

        fn locked() -> TournamentEntry {
            entry_with(EntryStatus::Locked, 10_000)
        }

        /// Same three costs, but hero 3 replaced by hero 4 -- one exchange.
        fn one_swap() -> Vec<RosterPick> {
            vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 2,
                    cost: 3_200,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 2_100,
                },
            ]
        }

        #[test]
        fn a_swap_within_the_allowance_and_the_budget_passes() {
            let violations = validate_swap(
                &held(),
                &one_swap(),
                &swap_tournament(2, 1),
                &locked(),
                0,
                false,
            );
            assert_eq!(rules(&violations), Vec::<RosterRule>::new());
        }

        /// Resubmitting the roster unchanged spends nothing, so it is legal
        /// even with no allowance at all. The service treats it as a no-op.
        #[test]
        fn an_unchanged_roster_spends_nothing() {
            let unchanged = picks(&[4_100, 3_200, 2_100]);
            let violations = validate_swap(
                &held(),
                &unchanged,
                &swap_tournament(2, 0),
                &locked(),
                0,
                false,
            );
            assert_eq!(rules(&violations), Vec::<RosterRule>::new());
        }

        #[test]
        fn a_closed_window_refuses_the_swap() {
            let shut = Tournament {
                swap_window_open: false,
                ..swap_tournament(2, 1)
            };
            let violations = validate_swap(&held(), &one_swap(), &shut, &locked(), 0, false);
            assert_eq!(rules(&violations), vec![RosterRule::SwapWindowClosed]);
        }

        /// A finished tournament's results are final, so the flag alone is not
        /// enough to reopen it.
        #[test]
        fn a_completed_tournament_refuses_even_with_the_window_open() {
            let over = Tournament {
                status: TournamentStatus::Completed,
                ..swap_tournament(2, 1)
            };
            let violations = validate_swap(&held(), &one_swap(), &over, &locked(), 0, false);
            assert_eq!(rules(&violations), vec![RosterRule::SwapWindowClosed]);
        }

        /// An unlocked entry has no committed roster to swap *from* -- it
        /// belongs on the draft path, where its picks are still free anyway.
        #[test]
        fn an_unlocked_entry_is_sent_back_to_the_draft_path() {
            let violations = validate_swap(
                &held(),
                &one_swap(),
                &swap_tournament(2, 1),
                &entry_with(EntryStatus::Draft, 10_000),
                0,
                false,
            );
            assert_eq!(rules(&violations), vec![RosterRule::SwapWindowClosed]);
        }

        #[test]
        fn a_window_grants_one_submission_not_a_running_budget() {
            let violations = validate_swap(
                &held(),
                &one_swap(),
                &swap_tournament(2, 2),
                &locked(),
                0,
                true,
            );
            assert_eq!(
                rules(&violations),
                vec![RosterRule::AlreadySwappedThisRound]
            );
        }

        #[test]
        fn exchanging_more_heroes_than_the_allowance_is_refused() {
            // Two heroes changed against an allowance of one.
            let two_changed = vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 3_200,
                },
                RosterPick {
                    hero_id: 5,
                    cost: 2_100,
                },
            ];
            let violations = validate_swap(
                &held(),
                &two_changed,
                &swap_tournament(2, 1),
                &locked(),
                0,
                false,
            );
            assert_eq!(rules(&violations), vec![RosterRule::SwapLimitExceeded]);
            assert_eq!(
                violations[0].message,
                "Swapping 2 heroes, but you have 1 swaps available."
            );
        }

        /// Carry-over is not merely arithmetic in a helper -- the policy has to
        /// actually spend it.
        #[test]
        fn a_carried_over_allowance_permits_a_bigger_exchange() {
            let two_changed = vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 3_200,
                },
                RosterPick {
                    hero_id: 5,
                    cost: 2_100,
                },
            ];
            // One per round, now in round 3, nothing spent: two banked.
            let violations = validate_swap(
                &held(),
                &two_changed,
                &swap_tournament(3, 1),
                &locked(),
                0,
                false,
            );
            assert_eq!(rules(&violations), Vec::<RosterRule>::new());
        }

        /// The budget is still the grant snapshotted on the entry, not the
        /// tournament's current one -- the same rule `validate_lock` follows.
        #[test]
        fn the_budget_comes_from_the_entry_not_the_tournament() {
            let generous_tournament = Tournament {
                credit_grant: 100_000,
                ..swap_tournament(2, 1)
            };
            let pricey = vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 2,
                    cost: 3_200,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 5_600,
                },
            ];
            let violations = validate_swap(
                &held(),
                &pricey,
                &generous_tournament,
                &entry_with(EntryStatus::Locked, 10_000),
                0,
                false,
            );
            assert_eq!(rules(&violations), vec![RosterRule::BudgetExceeded]);
        }

        /// A swap is an exchange, so unlike a draft it cannot leave the roster
        /// short -- dropping a hero without replacing it is not a swap.
        #[test]
        fn a_swap_may_not_shrink_the_roster() {
            let two_left = vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 2,
                    cost: 3_200,
                },
            ];
            let violations = validate_swap(
                &held(),
                &two_left,
                &swap_tournament(2, 1),
                &locked(),
                0,
                false,
            );
            assert_eq!(rules(&violations), vec![RosterRule::IncompleteRoster]);
        }

        #[test]
        fn a_swap_may_not_grow_the_roster_or_repeat_a_hero() {
            // Deliberately still inside the 10,000 grant, so `TooManyPicks` is
            // the only thing wrong with it.
            let four = picks(&[4_100, 3_200, 2_100, 500]);
            assert_eq!(
                rules(&validate_swap(
                    &held(),
                    &four,
                    &swap_tournament(2, 4),
                    &locked(),
                    0,
                    false
                )),
                vec![RosterRule::TooManyPicks]
            );

            let repeated = vec![
                RosterPick {
                    hero_id: 1,
                    cost: 4_100,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 3_200,
                },
                RosterPick {
                    hero_id: 4,
                    cost: 2_100,
                },
            ];
            assert_eq!(
                rules(&validate_swap(
                    &held(),
                    &repeated,
                    &swap_tournament(2, 4),
                    &locked(),
                    0,
                    false
                )),
                vec![RosterRule::DuplicateHero]
            );
        }

        #[test]
        fn every_broken_rule_is_reported_not_just_the_first() {
            let shut_and_over_budget = Tournament {
                swap_window_open: false,
                ..swap_tournament(2, 0)
            };
            let pricey_pair = vec![
                RosterPick {
                    hero_id: 4,
                    cost: 9_000,
                },
                RosterPick {
                    hero_id: 5,
                    cost: 8_000,
                },
            ];
            let violations = validate_swap(
                &held(),
                &pricey_pair,
                &shut_and_over_budget,
                &locked(),
                0,
                true,
            );

            assert_eq!(
                rules(&violations),
                vec![
                    RosterRule::SwapWindowClosed,
                    RosterRule::AlreadySwappedThisRound,
                    RosterRule::IncompleteRoster,
                    RosterRule::SwapLimitExceeded,
                    RosterRule::BudgetExceeded,
                ]
            );
        }
    }

    /// The service boundary: a typed rule becomes the string the frontend
    /// renders off `ApiError.violations[].rule`.
    #[test]
    fn violations_convert_to_the_wire_shape() {
        let violations = validate_lock(&picks(&[4_700, 3_200]), &tournament(), &entry());
        let wire: Vec<Violation> = violations.into_iter().map(Into::into).collect();

        assert_eq!(wire[0].rule, "INCOMPLETE_ROSTER");
        assert_eq!(
            wire[0].message,
            "Roster needs 3 heroes but only 2 selected."
        );
    }
}
