//! Roster roles: the role a manager gives each hero on their roster, and the
//! bonus that role earns from the hero's per-game stats.
//!
//! Named `RosterRole` rather than `HeroRole` because
//! [`crate::match_result::HeroRole`] already names something else -- the shape
//! a hero has inside one match (played, drafted, banned). This is a manager's
//! choice about a hero, not a fact about a match.
//!
//! **Nothing here stores a point**, exactly as in [`crate::scoring_engine`]: a
//! role's weights are mutable reference data retuned with a bare UPDATE, so
//! the bonus is priced on every read by [`crate::standings::board`].
//!
//! Assignments are an event log ([`RoleAssignment`]), not a column on a slot,
//! for the reason `roster_swaps` is: points stay in the round they were earned
//! in. A role re-assigned in round 3 must not re-price rounds 1 and 2, so the
//! fold asks [`role_at`] which role was in force for the round each game was
//! played in.

use crate::Violation;
use crate::match_metrics;
use crate::roster_policy::{RosterRule, RosterViolation};
use crate::rounding::round2;
use crate::scoring_rule_set_policy::is_well_formed;
use indexmap::IndexMap;
use rust_decimal::{Decimal, prelude::ToPrimitive};

/// The leaderboard column the role bonus is reported under.
///
/// Not a `scoring_coefficients` metric: no extractor implements it, so an
/// admin who prices `ROLE_BONUS` in a rule set gets the ordinary
/// unknown-metric warning and never a second column of the same name.
pub const ROLE_BONUS_METRIC: &str = "ROLE_BONUS";

/// One role in one tournament, with the stats it rewards.
///
/// `weights` keeps the admin's order, which is the order the hint lists them
/// in; the arithmetic does not depend on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRole {
    pub id: i64,
    pub name: String,
    /// At most this many heroes on one roster may take this role. `None` is
    /// uncapped.
    pub max_per_roster: Option<i32>,
    pub weights: IndexMap<String, Decimal>,
}

/// A tournament's roles, by id.
///
/// Its *absence* is what "roles are switched off" means at every call site:
/// the server reads `None` when `tournaments.roles_enabled` is false, so the
/// toggle is decided once, where the data is read, rather than re-checked by
/// every rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleBook {
    roles: IndexMap<i64, RosterRole>,
}

impl RoleBook {
    pub fn new(roles: impl IntoIterator<Item = RosterRole>) -> Self {
        Self {
            roles: roles.into_iter().map(|role| (role.id, role)).collect(),
        }
    }

    pub fn get(&self, role_id: i64) -> Option<&RosterRole> {
        self.roles.get(&role_id)
    }

    pub fn roles(&self) -> impl Iterator<Item = &RosterRole> {
        self.roles.values()
    }
}

/// From `from_round` on, this entry's hero plays `role_id`. A row of
/// `entry_hero_roles`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleAssignment {
    pub hero_id: i64,
    pub from_round: i32,
    pub role_id: i64,
}

/// The role in force for `hero_id` in `round`: the assignment with the greatest
/// `from_round` not past it.
///
/// `None` when the hero had no role yet -- it then earns no bonus, which is
/// also what a tournament that switched roles on mid-way gives every hero until
/// its manager picks one. Ownership is not this function's question: the
/// fold only asks about rounds a holding already covers, so a hero traded away
/// and re-acquired is priced under whatever role it arrived back with.
pub fn role_at(assignments: &[RoleAssignment], hero_id: i64, round: i32) -> Option<i64> {
    assignments
        .iter()
        .filter(|a| a.hero_id == hero_id && a.from_round <= round)
        .max_by_key(|a| a.from_round)
        .map(|a| a.role_id)
}

/// Each hero's role as it stands now -- i.e. in force from every round on.
pub fn current_roles(assignments: &[RoleAssignment]) -> IndexMap<i64, i64> {
    let mut latest: IndexMap<i64, RoleAssignment> = IndexMap::new();
    for assignment in assignments {
        let newer = latest
            .get(&assignment.hero_id)
            .is_none_or(|held| assignment.from_round >= held.from_round);
        if newer {
            latest.insert(assignment.hero_id, *assignment);
        }
    }
    latest
        .into_iter()
        .map(|(hero_id, a)| (hero_id, a.role_id))
        .collect()
}

/// What one game's stats are worth under one role.
///
/// Each stat's contribution is rounded **before** it is summed, the same rule
/// [`crate::scoring_engine::breakdown`] follows, so a displayed bonus is
/// exactly the sum of its parts. A stat the role does not weight is ignored,
/// and a weighted stat the game did not record scores zero.
pub fn role_bonus(role: &RosterRole, stats: &IndexMap<String, i32>) -> f64 {
    let total: f64 = role
        .weights
        .iter()
        .filter_map(|(stat, weight)| {
            let value = *stats.get(stat)?;
            let weight = weight
                .to_f64()
                .expect("a numeric(10,4) weight always fits an f64");
            Some(round2(f64::from(value) * weight))
        })
        .sum();
    round2(total)
}

/// Checks the roles a manager has given their roster.
///
/// `assignments` is `(hero_id, role_id)` for the roster as it would stand
/// after this submission. `require_complete` names the heroes that must have a
/// role: the whole roster at lock, the arrivals on a swap, and nobody while a
/// draft is still a scratchpad, where a hero without a role yet is not an
/// error.
///
/// Every broken rule is reported, not just the first, exactly as
/// [`crate::roster_policy`] does.
pub fn validate_assignments(
    roster_hero_ids: &[i64],
    assignments: &[(i64, i64)],
    book: Option<&RoleBook>,
    require_complete: &[i64],
) -> Vec<RosterViolation> {
    let mut violations = Vec::new();

    let Some(book) = book else {
        if !assignments.is_empty() {
            violations.push(RosterViolation::new(
                RosterRule::RolesDisabled,
                "This tournament does not use roles.",
            ));
        }
        return violations;
    };

    let mut unknown: Vec<i64> = assignments
        .iter()
        .map(|&(_, role_id)| role_id)
        .filter(|role_id| book.get(*role_id).is_none())
        .collect();
    unknown.sort();
    unknown.dedup();
    if !unknown.is_empty() {
        violations.push(RosterViolation::new(
            RosterRule::UnknownRole,
            format!("No role with id(s): {}.", join_ids(&unknown)),
        ));
    }

    let mut off_roster: Vec<i64> = Vec::new();
    let mut seen: Vec<i64> = Vec::new();
    for &(hero_id, _) in assignments {
        if !roster_hero_ids.contains(&hero_id) || seen.contains(&hero_id) {
            off_roster.push(hero_id);
        }
        seen.push(hero_id);
    }
    off_roster.sort();
    off_roster.dedup();
    if !off_roster.is_empty() {
        violations.push(RosterViolation::new(
            RosterRule::RoleHeroNotOnRoster,
            format!(
                "A role can only be given once to each hero on your roster (hero id(s): {}).",
                join_ids(&off_roster)
            ),
        ));
    }

    let mut unassigned: Vec<i64> = require_complete
        .iter()
        .copied()
        .filter(|hero_id| !assignments.iter().any(|&(h, _)| h == *hero_id))
        .collect();
    unassigned.sort();
    unassigned.dedup();
    if !unassigned.is_empty() {
        violations.push(RosterViolation::new(
            RosterRule::RoleUnassigned,
            format!(
                "Every hero needs a role (hero id(s) without one: {}).",
                join_ids(&unassigned)
            ),
        ));
    }

    // In the book's own order, so the message is stable.
    for role in book.roles() {
        let Some(cap) = role.max_per_roster else {
            continue;
        };
        let taken = assignments.iter().filter(|&&(_, r)| r == role.id).count();
        let taken = i32::try_from(taken).unwrap_or(i32::MAX);
        if taken > cap {
            violations.push(RosterViolation::new(
                RosterRule::RoleCapExceeded,
                format!("At most {cap} {} — {taken} assigned.", role.name),
            ));
        }
    }

    violations
}

fn join_ids(ids: &[i64]) -> String {
    ids.iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The rule vocabulary for a role's *definition*, as an admin submits it --
/// the counterpart to [`crate::scoring_rule_set_policy::ScoringRule`] for
/// coefficients, and checked the same way against the normalised stat name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleRule {
    /// Two weights on the same role price the same stat.
    DuplicateStat,

    /// A stat name is not SCREAMING_SNAKE_CASE, so the schema's format CHECK
    /// would reject it.
    MalformedStat,
}

impl RoleRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateStat => "DUPLICATE_STAT",
            Self::MalformedStat => "MALFORMED_STAT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleViolation {
    pub rule: RoleRule,
    pub message: String,
}

impl From<RoleViolation> for Violation {
    fn from(v: RoleViolation) -> Self {
        Violation::new(v.rule.as_str(), v.message)
    }
}

/// Normalises a stat name the same way a metric is -- trimmed and upper-cased
/// -- so `' attacks '` and `'Attacks'` are one stat on a role *and* match the
/// same column in a recorded game.
pub fn normalise_stat(stat: &str) -> String {
    match_metrics::normalise(stat)
}

/// Whether a (normalised) stat name would pass the schema's format CHECK.
pub fn is_well_formed_stat(stat: &str) -> bool {
    is_well_formed(stat)
}

/// Checks a role's weighted stats before the service saves them, so a bad
/// submission is a named 422 rather than the generic 409 a primary-key or
/// CHECK failure would produce further down.
///
/// Validates the *shape* of a stat name, never the set: which stats a game
/// records is whatever the tournament's stats sheet has columns for.
pub fn validate_weights(stats: &[String]) -> Vec<RoleViolation> {
    let mut violations = Vec::new();
    let normalised: Vec<String> = stats.iter().map(|s| normalise_stat(s)).collect();

    let mut malformed: Vec<&str> = Vec::new();
    for stat in &normalised {
        if !is_well_formed(stat) && !malformed.contains(&stat.as_str()) {
            malformed.push(stat);
        }
    }
    if !malformed.is_empty() {
        violations.push(RoleViolation {
            rule: RoleRule::MalformedStat,
            message: format!(
                "Stat name(s) must be letters, digits and underscores starting with a letter: {}.",
                quoted(&malformed)
            ),
        });
    }

    let mut counts: IndexMap<&str, usize> = IndexMap::new();
    for stat in &normalised {
        *counts.entry(stat.as_str()).or_default() += 1;
    }
    let mut duplicates: Vec<&str> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(stat, _)| stat)
        .collect();
    duplicates.sort();
    if !duplicates.is_empty() {
        violations.push(RoleViolation {
            rule: RoleRule::DuplicateStat,
            message: format!("Stat(s) weighted more than once: {}.", quoted(&duplicates)),
        });
    }

    violations
}

fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn role(id: i64, name: &str, cap: Option<i32>, weights: &[(&str, &str)]) -> RosterRole {
        RosterRole {
            id,
            name: name.into(),
            max_per_roster: cap,
            weights: weights
                .iter()
                .map(|(stat, w)| ((*stat).to_string(), Decimal::from_str(w).unwrap()))
                .collect(),
        }
    }

    fn book() -> RoleBook {
        RoleBook::new([
            role(
                1,
                "Attacker",
                None,
                &[("ATTACKS", "1.0000"), ("DAMAGE_DEALT", "0.2500")],
            ),
            role(2, "Healer", Some(1), &[("HEALING", "1.5000")]),
        ])
    }

    fn stats(pairs: &[(&str, i32)]) -> IndexMap<String, i32> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
    }

    fn assigned(hero_id: i64, from_round: i32, role_id: i64) -> RoleAssignment {
        RoleAssignment {
            hero_id,
            from_round,
            role_id,
        }
    }

    fn rules(violations: &[RosterViolation]) -> Vec<RosterRule> {
        violations.iter().map(|v| v.rule).collect()
    }

    // -- role_at ------------------------------------------------------------

    #[test]
    fn the_role_in_force_is_the_latest_assignment_not_past_the_round() {
        let log = [assigned(7, 1, 1), assigned(7, 3, 2)];

        assert_eq!(role_at(&log, 7, 1), Some(1));
        assert_eq!(role_at(&log, 7, 2), Some(1));
        assert_eq!(role_at(&log, 7, 3), Some(2), "re-assigned from round 3");
        assert_eq!(role_at(&log, 7, 9), Some(2));
    }

    #[test]
    fn a_hero_with_no_assignment_yet_has_no_role() {
        let log = [assigned(7, 3, 1)];

        assert_eq!(role_at(&log, 7, 2), None, "the role arrives in round 3");
        assert_eq!(role_at(&log, 9, 5), None, "never assigned");
    }

    #[test]
    fn the_log_order_does_not_matter() {
        let log = [assigned(7, 4, 2), assigned(7, 1, 1)];
        assert_eq!(role_at(&log, 7, 2), Some(1));
        assert_eq!(role_at(&log, 7, 4), Some(2));
    }

    #[test]
    fn current_roles_reads_each_heros_latest_assignment() {
        let log = [assigned(7, 1, 1), assigned(9, 1, 2), assigned(7, 3, 2)];
        let current = current_roles(&log);

        assert_eq!(current.get(&7), Some(&2));
        assert_eq!(current.get(&9), Some(&2));
    }

    // -- role_bonus ---------------------------------------------------------

    #[test]
    fn the_bonus_prices_the_stats_the_role_weights() {
        let attacker = book().get(1).unwrap().clone();
        // 5 * 1.0 + 9 * 0.25 = 7.25; HEALING is not an Attacker stat.
        assert_eq!(
            role_bonus(
                &attacker,
                &stats(&[("ATTACKS", 5), ("DAMAGE_DEALT", 9), ("HEALING", 4)])
            ),
            7.25
        );
    }

    #[test]
    fn a_weighted_stat_the_game_did_not_record_scores_nothing() {
        let healer = book().get(2).unwrap().clone();
        assert_eq!(role_bonus(&healer, &stats(&[("ATTACKS", 5)])), 0.0);
        assert_eq!(role_bonus(&healer, &IndexMap::new()), 0.0);
    }

    #[test]
    fn each_contribution_is_rounded_before_it_is_summed() {
        let odd = role(
            1,
            "Odd",
            None,
            &[("ATTACKS", "0.3330"), ("HEALING", "0.3330")],
        );
        // 11 * 0.333 = 3.663 -> 3.66, twice; 7.32 rather than round2(7.326) = 7.33.
        assert_eq!(
            role_bonus(&odd, &stats(&[("ATTACKS", 11), ("HEALING", 11)])),
            7.32
        );
    }

    #[test]
    fn a_negative_weight_is_a_penalty() {
        let reckless = role(1, "Reckless", None, &[("DAMAGE_TAKEN", "-0.5000")]);
        assert_eq!(role_bonus(&reckless, &stats(&[("DAMAGE_TAKEN", 6)])), -3.0);
    }

    // -- validate_assignments -----------------------------------------------

    #[test]
    fn a_complete_valid_assignment_passes() {
        let violations = validate_assignments(&[7, 9], &[(7, 1), (9, 2)], Some(&book()), &[7, 9]);
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn roles_on_a_tournament_without_them_are_refused() {
        assert_eq!(
            rules(&validate_assignments(&[7], &[(7, 1)], None, &[])),
            vec![RosterRule::RolesDisabled]
        );
        assert!(
            validate_assignments(&[7], &[], None, &[7]).is_empty(),
            "with roles off, nothing is required either"
        );
    }

    #[test]
    fn an_unknown_role_and_an_off_roster_hero_are_both_reported() {
        let violations = validate_assignments(&[7], &[(7, 99), (8, 1)], Some(&book()), &[]);
        assert_eq!(
            rules(&violations),
            vec![RosterRule::UnknownRole, RosterRule::RoleHeroNotOnRoster]
        );
    }

    #[test]
    fn the_same_hero_given_two_roles_is_refused() {
        let violations = validate_assignments(&[7], &[(7, 1), (7, 2)], Some(&book()), &[]);
        assert_eq!(rules(&violations), vec![RosterRule::RoleHeroNotOnRoster]);
    }

    #[test]
    fn a_draft_may_leave_heroes_without_a_role_but_a_commit_may_not() {
        assert!(validate_assignments(&[7, 9], &[(7, 1)], Some(&book()), &[]).is_empty());
        let violations = validate_assignments(&[7, 9], &[(7, 1)], Some(&book()), &[7, 9]);
        assert_eq!(rules(&violations), vec![RosterRule::RoleUnassigned]);
        assert!(
            violations[0].message.contains('9'),
            "{}",
            violations[0].message
        );
    }

    #[test]
    fn a_role_over_its_cap_is_refused() {
        let violations = validate_assignments(&[7, 9], &[(7, 2), (9, 2)], Some(&book()), &[]);
        assert_eq!(rules(&violations), vec![RosterRule::RoleCapExceeded]);
        assert!(
            violations[0].message.contains("Healer"),
            "{}",
            violations[0].message
        );
    }

    #[test]
    fn an_uncapped_role_takes_the_whole_roster() {
        assert!(
            validate_assignments(&[7, 9, 11], &[(7, 1), (9, 1), (11, 1)], Some(&book()), &[])
                .is_empty()
        );
    }

    // -- validate_weights ---------------------------------------------------

    #[test]
    fn weights_are_checked_against_the_normalised_stat() {
        let violations =
            validate_weights(&[" attacks ".into(), "Attacks".into(), "damage-dealt".into()]);
        let found: Vec<RoleRule> = violations.iter().map(|v| v.rule).collect();
        assert_eq!(
            found,
            vec![RoleRule::MalformedStat, RoleRule::DuplicateStat]
        );
        assert!(violations[0].message.contains("'DAMAGE-DEALT'"));
        assert!(violations[1].message.contains("'ATTACKS'"));
    }

    #[test]
    fn well_formed_distinct_weights_pass() {
        assert!(validate_weights(&["ATTACKS".into(), "HEALING_2".into()]).is_empty());
    }
}
