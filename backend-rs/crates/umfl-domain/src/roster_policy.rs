//! The league's roster rules, as pure functions.
//!
//! A direct port of `tournament/RosterPolicy.kt`.
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
/// On a non-positive `credit_grant`, mirroring the Kotlin's
/// `require(creditGrant > 0)`. That `IllegalArgumentException` has no entry in
/// `GlobalExceptionHandler`, so it reaches the catch-all as a 500; a panic here
/// reaches `CatchPanicLayer` as the same 500.
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

    let selected = i32::try_from(picks.len()).unwrap_or(i32::MAX);
    if selected > tournament.roster_size {
        violations.push(RosterViolation::new(
            RosterRule::TooManyPicks,
            format!(
                "Selected {selected} heroes but the roster size is {}.",
                tournament.roster_size
            ),
        ));
    }

    // `groupingBy { it.heroId }.eachCount().filterValues { it > 1 }.keys`, then
    // `.sorted()`. IndexMap keeps encounter order (PORTING.md §4.2), but the
    // sort is what actually fixes the message, exactly as in the Kotlin.
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

    if !duplicates.is_empty() {
        let ids = duplicates
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(RosterViolation::new(
            RosterRule::DuplicateHero,
            format!("A hero may only be selected once (repeated ids: {ids})."),
        ));
    }

    violations
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

    let selected = i32::try_from(picks.len()).unwrap_or(i32::MAX);
    if selected < tournament.roster_size {
        violations.push(RosterViolation::new(
            RosterRule::IncompleteRoster,
            format!(
                "Roster needs {} heroes but only {selected} selected.",
                tournament.roster_size
            ),
        ));
    }

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

/// The roster rules, exercised directly -- a near-1:1 port of `RosterPolicyTest`.
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
        }
    }

    fn tournament() -> Tournament {
        tournament_with(TournamentStatus::RegistrationOpen, 3, 10_000)
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

    /// The Kotlin's `picks(vararg costs)`: hero ids are 1-based positions, so
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

        /// The Kotlin's `require(creditGrant > 0)`. Both runtimes answer 500.
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
