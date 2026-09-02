//! The rules a recorded match has to satisfy before anything is written.
//!
//! A direct port of `match/MatchResultPolicy.kt`.
//!
//! Pre-validates an admin's submission so a bad one comes back as a clear 422
//! naming every broken rule, instead of a raw constraint violation from a
//! partial unique index or a composite foreign key. Deliberately free of
//! persistence, exactly like [`crate::roster_policy`]: everything it needs --
//! the tournament's legal map ids, and the hero ids that actually exist -- is
//! resolved by the caller and passed in.
//!
//! Two of these rules carry the league's no-draw invariant, and they are the
//! ones to know before touching this file: [`MatchRule::NotExactlyOneWinner`]
//! treats zero winners as being as wrong as two, and
//! [`MatchRule::LoserHasPositiveHealth`] rejects a loser who survived.

use crate::Violation;
use crate::match_result::BanType;
use indexmap::IndexMap;
use std::collections::BTreeSet;

/// The match rule vocabulary, one constant per way a submission can be wrong.
///
/// The `rule` field the frontend reads off `ApiError.violations` is the
/// constant name, so [`MatchRule::as_str`] is wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MatchRule {
    /// One or more games use a map outside this tournament's board pool.
    MapNotInPool,

    /// The series has no games at all -- at least one is required.
    InvalidGameCount,

    /// Game numbers aren't exactly 1..N with no gaps or repeats.
    GameNumbersNotSequential,

    /// The series does not have exactly the expected number of sides.
    InvalidParticipantCount,

    /// One or more games don't have exactly the expected number of sides.
    InvalidGameParticipantCount,

    /// The same hero appears on both sides within one game.
    DuplicateHero,

    /// The same hero is banned more than once.
    DuplicateBan,

    /// A ban names a side that cannot exist -- one outside 0..=1, or any side
    /// at all on a `PRE_BAN`, which is struck before sides are assigned. A
    /// typed ban with *no* side is deliberately not a violation: every row
    /// recorded before `hero_bans.side` existed looks like that, and rejecting
    /// them would make an already-recorded match uncorrectable.
    BanSideInvalid,

    /// The same hero is drafted more than once by one side.
    DuplicatePick,

    /// A banned hero was also played, somewhere in the series.
    BannedHeroPlayed,

    /// A banned hero was also drafted -- it was struck before either side
    /// could take it.
    BannedHeroDrafted,

    /// A hero played a game for a side that never drafted it. A recorded draft
    /// is the complete list of what a side brought, so every hero it fielded
    /// has to be on it.
    PlayedHeroNotDrafted,

    /// One or more games are not flagged with exactly one winner. Every game is
    /// played to a decision -- there is no draw in this league, so zero winners
    /// is as wrong as two.
    NotExactlyOneWinner,

    /// A losing hero finished with positive health.
    LoserHasPositiveHealth,

    /// A `hero_id` referenced by a game participant, a pick or a ban does not
    /// exist.
    UnknownHero,
}

impl MatchRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MapNotInPool => "MAP_NOT_IN_POOL",
            Self::InvalidGameCount => "INVALID_GAME_COUNT",
            Self::GameNumbersNotSequential => "GAME_NUMBERS_NOT_SEQUENTIAL",
            Self::InvalidParticipantCount => "INVALID_PARTICIPANT_COUNT",
            Self::InvalidGameParticipantCount => "INVALID_GAME_PARTICIPANT_COUNT",
            Self::DuplicateHero => "DUPLICATE_HERO",
            Self::DuplicateBan => "DUPLICATE_BAN",
            Self::BanSideInvalid => "BAN_SIDE_INVALID",
            Self::DuplicatePick => "DUPLICATE_PICK",
            Self::BannedHeroPlayed => "BANNED_HERO_PLAYED",
            Self::BannedHeroDrafted => "BANNED_HERO_DRAFTED",
            Self::PlayedHeroNotDrafted => "PLAYED_HERO_NOT_DRAFTED",
            Self::NotExactlyOneWinner => "NOT_EXACTLY_ONE_WINNER",
            Self::LoserHasPositiveHealth => "LOSER_HAS_POSITIVE_HEALTH",
            Self::UnknownHero => "UNKNOWN_HERO",
        }
    }
}

impl std::fmt::Display for MatchRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One broken match rule.
///
/// Kept as its own type rather than [`Violation`] so tests can compare rules
/// without string matching; it converts at the service boundary, which is what
/// lets this module define its vocabulary without touching `error.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchViolation {
    pub rule: MatchRule,
    pub message: String,
}

impl MatchViolation {
    fn new(rule: MatchRule, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
        }
    }
}

impl From<MatchViolation> for Violation {
    fn from(v: MatchViolation) -> Self {
        Violation::new(v.rule.as_str(), v.message)
    }
}

/// One human's side of the series, as submitted.
///
/// `player_label` is who piloted this side for the whole series, as free text
/// -- there is no `player` table to check it against, and nothing scores it.
/// Nothing validates it here for the same reason: any string, including none
/// at all, is a legal answer.
///
/// `drafted_hero_ids` is this side's half of the match's draft -- every hero it
/// took, whether or not it fielded one. The side is the *position* of this
/// input in the participants list, exactly as it already is for `player_label`,
/// so an out-of-range side is unrepresentable and there is no rule policing
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchParticipantInput {
    pub player_label: Option<String>,
    pub drafted_hero_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchGameParticipantInput {
    pub hero_id: i64,
    pub health_remaining: i32,
    pub is_winner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchGameInput {
    pub game_number: i32,
    pub map_id: i64,
    pub participants: Vec<MatchGameParticipantInput>,
}

/// A hero struck out of the series, as submitted.
///
/// `side` is whose draft the hero was struck out of, not who struck it --
/// `ban_type` already says that. `None` means either a `PRE_BAN` (there are no
/// sides yet) or a ban recorded before the column existed; both are legal, see
/// [`MatchRule::BanSideInvalid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchBanInput {
    pub hero_id: i64,
    pub ban_type: BanType,
    pub side: Option<i32>,
}

/// Two sides to a series. Kotlin carries this as a defaulted parameter that no
/// call site has ever overridden; [`validate_expecting`] is the parameterised
/// form, kept so the default stays a default rather than becoming a constant
/// baked into the arithmetic.
pub const EXPECTED_PARTICIPANT_COUNT: usize = 2;

/// Validates a match submission, reporting **every** broken rule rather than
/// the first, so the admin wizard can highlight everything wrong in one pass.
pub fn validate(
    valid_map_ids: &BTreeSet<i64>,
    valid_hero_ids: &BTreeSet<i64>,
    participants: &[MatchParticipantInput],
    games: &[MatchGameInput],
    bans: &[MatchBanInput],
) -> Vec<MatchViolation> {
    validate_expecting(
        valid_map_ids,
        valid_hero_ids,
        participants,
        games,
        bans,
        EXPECTED_PARTICIPANT_COUNT,
    )
}

pub fn validate_expecting(
    valid_map_ids: &BTreeSet<i64>,
    valid_hero_ids: &BTreeSet<i64>,
    participants: &[MatchParticipantInput],
    games: &[MatchGameInput],
    bans: &[MatchBanInput],
    expected_participant_count: usize,
) -> Vec<MatchViolation> {
    let mut violations = Vec::new();

    if participants.len() != expected_participant_count {
        violations.push(MatchViolation::new(
            MatchRule::InvalidParticipantCount,
            format!(
                "Expected {expected_participant_count} participants but got {}.",
                participants.len()
            ),
        ));
    }

    if games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::InvalidGameCount,
            "At least one game is required.",
        ));
    } else {
        let mut numbers: Vec<i32> = games.iter().map(|g| g.game_number).collect();
        numbers.sort();
        let expected: Vec<i32> = (1..=games.len() as i32).collect();
        if numbers != expected {
            violations.push(MatchViolation::new(
                MatchRule::GameNumbersNotSequential,
                format!(
                    "Game numbers must be exactly 1..{} with no gaps or repeats.",
                    games.len()
                ),
            ));
        }
    }

    let bad_map_games = game_numbers(games, |g| !valid_map_ids.contains(&g.map_id));
    if !bad_map_games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::MapNotInPool,
            format!(
                "Game(s) {} use a map that is not in this tournament's board pool.",
                render_numbers(&bad_map_games)
            ),
        ));
    }

    let bad_count_games = game_numbers(games, |g| {
        g.participants.len() != expected_participant_count
    });
    if !bad_count_games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::InvalidGameParticipantCount,
            format!(
                "Game(s) {} don't have exactly {expected_participant_count} sides.",
                render_numbers(&bad_count_games)
            ),
        ));
    }

    let same_hero_games = game_numbers(games, |g| {
        let distinct: BTreeSet<i64> = g.participants.iter().map(|p| p.hero_id).collect();
        distinct.len() != g.participants.len()
    });
    if !same_hero_games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::DuplicateHero,
            format!(
                "Game(s) {} have the same hero on both sides.",
                render_numbers(&same_hero_games)
            ),
        ));
    }

    let duplicate_bans = repeated(bans.iter().map(|b| b.hero_id));
    if !duplicate_bans.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::DuplicateBan,
            format!(
                "Hero(es) banned more than once: {}.",
                render_ids(&duplicate_bans)
            ),
        ));
    }

    let bad_ban_sides: Vec<&MatchBanInput> = bans
        .iter()
        .filter(|ban| match ban.side {
            Some(side) => !(0..=1).contains(&side) || ban.ban_type == BanType::PreBan,
            None => false,
        })
        .collect();
    if !bad_ban_sides.is_empty() {
        let listed = bad_ban_sides
            .iter()
            .map(|b| {
                format!(
                    "hero {} ({}, side {})",
                    b.hero_id,
                    b.ban_type,
                    // Every element here matched `Some`, so the fallback is
                    // unreachable rather than a rendering decision.
                    b.side.map_or_else(String::new, |s| s.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(MatchViolation::new(
            MatchRule::BanSideInvalid,
            format!(
                "Ban(s) with an impossible side: {listed}. \
                 A side is 0 or 1, and a PRE_BAN precedes both."
            ),
        ));
    }

    // Per side, then unioned: the same hero doubled by both sides is one entry,
    // matching Kotlin's `flatMap { ... }.toSortedSet()`.
    let duplicate_picks: BTreeSet<i64> = participants
        .iter()
        .flat_map(|p| repeated(p.drafted_hero_ids.iter().copied()))
        .collect();
    if !duplicate_picks.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::DuplicatePick,
            format!(
                "Hero(es) drafted more than once by one side: {}.",
                render_ids(&duplicate_picks)
            ),
        ));
    }

    let drafted_hero_ids: BTreeSet<i64> = participants
        .iter()
        .flat_map(|p| p.drafted_hero_ids.iter().copied())
        .collect();
    let banned_but_drafted: BTreeSet<i64> = bans
        .iter()
        .map(|b| b.hero_id)
        .filter(|id| drafted_hero_ids.contains(id))
        .collect();
    if !banned_but_drafted.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::BannedHeroDrafted,
            format!(
                "Hero(es) both banned and drafted in this series: {}.",
                render_ids(&banned_but_drafted)
            ),
        ));
    }

    // `side` is the list position on both sides of this check -- of the
    // participants list for the draft, and of a game's participants list for
    // who fielded the hero. A game with the wrong number of sides is
    // InvalidGameParticipantCount's problem, so an unmatched index here is
    // simply skipped rather than reported twice.
    let undrafted_plays: BTreeSet<(i32, i64)> = games
        .iter()
        .flat_map(|game| {
            game.participants
                .iter()
                .enumerate()
                .filter_map(move |(side, played)| {
                    let draft = &participants.get(side)?.drafted_hero_ids;
                    if draft.contains(&played.hero_id) {
                        None
                    } else {
                        Some((game.game_number, played.hero_id))
                    }
                })
        })
        .collect();
    if !undrafted_plays.is_empty() {
        let listed = undrafted_plays
            .iter()
            .map(|(game_number, hero_id)| format!("{hero_id} in game {game_number}"))
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(MatchViolation::new(
            MatchRule::PlayedHeroNotDrafted,
            format!(
                "Hero(es) played by a side that did not draft them: {listed}. \
                 A recorded draft lists every hero a side brought, fielded or not."
            ),
        ));
    }

    let played_hero_ids: BTreeSet<i64> = games
        .iter()
        .flat_map(|g| g.participants.iter().map(|p| p.hero_id))
        .collect();
    let banned_but_played: BTreeSet<i64> = bans
        .iter()
        .map(|b| b.hero_id)
        .filter(|id| played_hero_ids.contains(id))
        .collect();
    if !banned_but_played.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::BannedHeroPlayed,
            format!(
                "Hero(es) both banned and played somewhere in this series: {}.",
                render_ids(&banned_but_played)
            ),
        ));
    }

    let undecided_games = game_numbers(games, |g| {
        g.participants.iter().filter(|p| p.is_winner).count() != 1
    });
    if !undecided_games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::NotExactlyOneWinner,
            format!(
                "Game(s) {} do not have exactly one winner. Every game is \
                 played to a decision, so a game with no winner is as invalid as one with two.",
                render_numbers(&undecided_games)
            ),
        ));
    }

    let surviving_loser_games = game_numbers(games, |g| {
        g.participants
            .iter()
            .any(|p| !p.is_winner && p.health_remaining > 0)
    });
    if !surviving_loser_games.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::LoserHasPositiveHealth,
            format!(
                "Losing hero(es) in game(s) {} must have 0 or less health.",
                render_numbers(&surviving_loser_games)
            ),
        ));
    }

    let unknown_heroes: BTreeSet<i64> = games
        .iter()
        .flat_map(|g| g.participants.iter().map(|p| p.hero_id))
        .chain(
            participants
                .iter()
                .flat_map(|p| p.drafted_hero_ids.iter().copied()),
        )
        .chain(bans.iter().map(|b| b.hero_id))
        .filter(|id| !valid_hero_ids.contains(id))
        .collect();
    if !unknown_heroes.is_empty() {
        violations.push(MatchViolation::new(
            MatchRule::UnknownHero,
            format!("Hero(es) do not exist: {}.", render_ids(&unknown_heroes)),
        ));
    }

    violations
}

/// The ascending game numbers of every game matching `predicate`.
fn game_numbers(games: &[MatchGameInput], predicate: impl Fn(&MatchGameInput) -> bool) -> Vec<i32> {
    let mut numbers: Vec<i32> = games
        .iter()
        .filter(|g| predicate(g))
        .map(|g| g.game_number)
        .collect();
    numbers.sort();
    numbers
}

/// The values appearing more than once, ascending. Kotlin reaches this through
/// `groupingBy { it }.eachCount().filterValues { it > 1 }.keys`; the result is
/// a set either way, and every caller sorts it.
fn repeated(values: impl Iterator<Item = i64>) -> BTreeSet<i64> {
    let mut counts: IndexMap<i64, usize> = IndexMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|&(_, count)| count > 1)
        .map(|(value, _)| value)
        .collect()
}

/// Kotlin renders a `List<Int>` inside a string template as `[1, 2]`, brackets
/// included, and the tests assert on those brackets. `joinToString()` -- used
/// for the id lists -- does not add them.
fn render_numbers(numbers: &[i32]) -> String {
    format!(
        "[{}]",
        numbers
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn render_ids(ids: &BTreeSet<i64>) -> String {
    ids.iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_maps() -> BTreeSet<i64> {
        BTreeSet::from([1, 2, 3])
    }

    fn valid_heroes() -> BTreeSet<i64> {
        BTreeSet::from([10, 11, 12])
    }

    /// Both sides draft both heroes by default. A real draft is usually
    /// disjoint, but these fixtures swap 10 and 11 across sides from game to
    /// game, and a recorded draft has to cover every hero the side fielded --
    /// so the default keeps every test about the rule it is actually testing.
    fn participants() -> Vec<MatchParticipantInput> {
        drafts(&[10, 11], &[10, 11])
    }

    fn drafts(drafted1: &[i64], drafted2: &[i64]) -> Vec<MatchParticipantInput> {
        vec![
            MatchParticipantInput {
                player_label: Some("Someone".into()),
                drafted_hero_ids: drafted1.to_vec(),
            },
            MatchParticipantInput {
                player_label: Some("Someone Else".into()),
                drafted_hero_ids: drafted2.to_vec(),
            },
        ]
    }

    fn played(hero_id: i64) -> MatchGameParticipantInput {
        MatchGameParticipantInput {
            hero_id,
            health_remaining: 0,
            is_winner: false,
        }
    }

    fn won(hero_id: i64) -> MatchGameParticipantInput {
        MatchGameParticipantInput {
            is_winner: true,
            ..played(hero_id)
        }
    }

    fn with_health(mut p: MatchGameParticipantInput, health: i32) -> MatchGameParticipantInput {
        p.health_remaining = health;
        p
    }

    fn game(number: i32, participants: Vec<MatchGameParticipantInput>) -> MatchGameInput {
        MatchGameInput {
            game_number: number,
            map_id: 1,
            participants,
        }
    }

    fn on_map(mut g: MatchGameInput, map_id: i64) -> MatchGameInput {
        g.map_id = map_id;
        g
    }

    fn one_legal_game(hero_a: i64, hero_b: i64) -> Vec<MatchGameInput> {
        vec![game(1, vec![won(hero_a), played(hero_b)])]
    }

    fn ban(hero_id: i64, ban_type: BanType, side: Option<i32>) -> MatchBanInput {
        MatchBanInput {
            hero_id,
            ban_type,
            side,
        }
    }

    /// The rules broken, in the order the policy reports them.
    fn rules(violations: &[MatchViolation]) -> Vec<MatchRule> {
        violations.iter().map(|v| v.rule).collect()
    }

    fn rule_set(violations: &[MatchViolation]) -> BTreeSet<MatchRule> {
        violations.iter().map(|v| v.rule).collect()
    }

    fn check(
        participants: &[MatchParticipantInput],
        games: &[MatchGameInput],
        bans: &[MatchBanInput],
    ) -> Vec<MatchViolation> {
        validate(&valid_maps(), &valid_heroes(), participants, games, bans)
    }

    fn only_message(violations: &[MatchViolation]) -> &str {
        assert_eq!(
            violations.len(),
            1,
            "expected one violation: {violations:?}"
        );
        &violations[0].message
    }

    #[test]
    fn a_legal_single_game_match_with_a_winner_has_no_violations() {
        let violations = check(&participants(), &one_legal_game(10, 11), &[]);
        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_game_with_no_winner_is_rejected_every_game_is_played_to_a_decision() {
        let violations = check(
            &participants(),
            &[game(1, vec![played(10), played(11)])],
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::NotExactlyOneWinner]);
        assert!(only_message(&violations).contains("[1]"));
    }

    #[test]
    fn a_game_with_a_positive_health_loser_is_rejected() {
        let violations = check(
            &participants(),
            &[game(
                1,
                vec![with_health(won(10), 7), with_health(played(11), 5)],
            )],
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::LoserHasPositiveHealth]);
        assert!(only_message(&violations).contains("[1]"));
    }

    #[test]
    fn a_winner_with_negative_health_is_legal() {
        let violations = check(
            &participants(),
            &[game(1, vec![with_health(won(10), -2), played(11)])],
            &[],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_best_of_three_series_with_a_hero_repeated_across_games_is_legal() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), played(11)]),
                game(2, vec![won(11), played(10)]),
                game(3, vec![won(10), played(11)]),
            ],
            &[],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_map_outside_the_tournaments_pool_is_rejected_naming_the_offending_game() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), played(11)]),
                on_map(game(2, vec![won(11), played(10)]), 99),
            ],
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::MapNotInPool]);
        assert!(only_message(&violations).contains("[2]"));
    }

    #[test]
    fn too_few_participants_is_rejected() {
        let violations = check(
            &[MatchParticipantInput {
                player_label: Some("Someone".into()),
                drafted_hero_ids: vec![10, 11],
            }],
            &one_legal_game(10, 11),
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::InvalidParticipantCount]);
    }

    #[test]
    fn too_many_participants_is_rejected() {
        let mut three = drafts(&[10, 11], &[10, 11]);
        three.push(MatchParticipantInput {
            player_label: Some("C".into()),
            drafted_hero_ids: vec![10, 11],
        });

        let violations = check(&three, &one_legal_game(10, 11), &[]);

        assert_eq!(rules(&violations), vec![MatchRule::InvalidParticipantCount]);
    }

    #[test]
    fn zero_games_is_rejected() {
        let violations = check(&participants(), &[], &[]);
        assert_eq!(rules(&violations), vec![MatchRule::InvalidGameCount]);
    }

    #[test]
    fn game_numbers_with_a_gap_are_rejected() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), played(11)]),
                game(3, vec![won(11), played(10)]),
            ],
            &[],
        );

        assert_eq!(
            rules(&violations),
            vec![MatchRule::GameNumbersNotSequential]
        );
    }

    #[test]
    fn repeated_game_numbers_are_rejected() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), played(11)]),
                game(1, vec![won(11), played(10)]),
            ],
            &[],
        );

        assert_eq!(
            rules(&violations),
            vec![MatchRule::GameNumbersNotSequential]
        );
    }

    #[test]
    fn the_same_hero_on_both_sides_within_one_game_is_rejected() {
        let violations = check(&participants(), &[game(1, vec![won(10), played(10)])], &[]);

        assert_eq!(rules(&violations), vec![MatchRule::DuplicateHero]);
        assert!(only_message(&violations).contains("[1]"));
    }

    #[test]
    fn a_game_with_the_wrong_number_of_sides_is_rejected() {
        let violations = check(&participants(), &[game(1, vec![won(10)])], &[]);

        assert_eq!(
            rules(&violations),
            vec![MatchRule::InvalidGameParticipantCount]
        );
    }

    #[test]
    fn a_hero_banned_then_played_in_a_later_game_is_rejected() {
        // Drafted too, since a side cannot field a hero it never drafted --
        // which is why a banned hero that played breaks both ban rules at once
        // rather than only BannedHeroPlayed.
        let violations = check(
            &drafts(&[10, 11, 12], &[10, 11]),
            &[
                game(1, vec![won(10), played(11)]),
                game(2, vec![won(12), played(11)]),
            ],
            &[ban(12, BanType::PreBan, None)],
        );

        assert_eq!(
            rule_set(&violations),
            BTreeSet::from([MatchRule::BannedHeroDrafted, MatchRule::BannedHeroPlayed])
        );
        assert!(violations.iter().all(|v| v.message.contains("12")));
    }

    #[test]
    fn a_hero_drafted_and_never_fielded_is_legal_that_is_what_a_draft_records() {
        let violations = check(&drafts(&[10, 12], &[11]), &one_legal_game(10, 11), &[]);

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_hero_played_by_a_side_that_did_not_draft_it_is_rejected_naming_hero_and_game() {
        let violations = check(
            &drafts(&[10], &[11]),
            &[
                game(1, vec![won(10), played(11)]),
                game(2, vec![won(12), played(11)]),
            ],
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::PlayedHeroNotDrafted]);
        assert!(only_message(&violations).contains("12 in game 2"));
    }

    #[test]
    fn drafting_a_hero_for_one_side_does_not_let_the_other_side_field_it() {
        let violations = check(&drafts(&[10, 11], &[12]), &one_legal_game(10, 11), &[]);

        assert_eq!(rules(&violations), vec![MatchRule::PlayedHeroNotDrafted]);
        assert!(only_message(&violations).contains("11 in game 1"));
    }

    #[test]
    fn the_same_hero_drafted_twice_by_one_side_is_rejected() {
        let violations = check(
            &drafts(&[10, 10, 11], &[10, 11]),
            &one_legal_game(10, 11),
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::DuplicatePick]);
        assert!(only_message(&violations).contains("10"));
    }

    #[test]
    fn the_same_hero_drafted_by_both_sides_is_legal_sides_may_trade_a_hero_between_games() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), played(11)]),
                game(2, vec![won(11), played(10)]),
            ],
            &[],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_banned_hero_that_was_also_drafted_is_rejected() {
        let violations = check(
            &drafts(&[10, 11, 12], &[10, 11]),
            &one_legal_game(10, 11),
            &[ban(12, BanType::OpponentBan, Some(0))],
        );

        assert_eq!(rules(&violations), vec![MatchRule::BannedHeroDrafted]);
        assert!(only_message(&violations).contains("12"));
    }

    #[test]
    fn a_nonexistent_hero_id_on_a_draft_is_rejected() {
        let violations = check(
            &drafts(&[10, 11, 999], &[10, 11]),
            &one_legal_game(10, 11),
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::UnknownHero]);
        assert!(only_message(&violations).contains("999"));
    }

    #[test]
    fn the_same_hero_banned_twice_is_rejected() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[
                ban(12, BanType::PreBan, None),
                ban(12, BanType::SelfBan, Some(0)),
            ],
        );

        assert_eq!(rules(&violations), vec![MatchRule::DuplicateBan]);
    }

    #[test]
    fn a_typed_ban_carries_the_side_whose_draft_it_came_out_of() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::OpponentBan, Some(0))],
        );
        let other_side = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::SelfBan, Some(1))],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
        assert!(other_side.is_empty(), "expected none, got {other_side:?}");
    }

    #[test]
    fn a_pre_ban_with_no_side_is_legal_it_is_struck_before_sides_exist() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::PreBan, None)],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_pre_ban_that_names_a_side_is_rejected() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::PreBan, Some(0))],
        );

        assert_eq!(rules(&violations), vec![MatchRule::BanSideInvalid]);
        assert!(only_message(&violations).contains("hero 12 (PRE_BAN, side 0)"));
    }

    #[test]
    fn a_ban_naming_a_side_outside_0_and_1_is_rejected() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::SelfBan, Some(2))],
        );

        assert_eq!(rules(&violations), vec![MatchRule::BanSideInvalid]);
        assert!(only_message(&violations).contains("side 2"));
    }

    /// Every `hero_bans` row written before V7 added the column looks like this.
    /// Rejecting it would make an already-recorded match uncorrectable, which
    /// is why BanSideInvalid polices only an impossible side, never a missing
    /// one.
    #[test]
    fn a_typed_ban_with_no_side_at_all_is_legal_so_recorded_matches_stay_correctable() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::OpponentBan, None)],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn a_hero_banned_in_one_match_and_played_in_another_is_legal() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(12, BanType::OpponentBan, Some(0))],
        );

        assert!(violations.is_empty(), "expected none, got {violations:?}");
    }

    #[test]
    fn two_winners_in_one_game_is_rejected_without_tripping_another_games_legal_winner() {
        let violations = check(
            &participants(),
            &[
                game(1, vec![won(10), won(11)]),
                game(2, vec![won(11), played(10)]),
            ],
            &[],
        );

        assert_eq!(rules(&violations), vec![MatchRule::NotExactlyOneWinner]);
        assert!(only_message(&violations).contains("[1]"));
    }

    #[test]
    fn a_nonexistent_hero_id_on_a_game_participant_is_rejected() {
        let violations = check(&drafts(&[10], &[999]), &one_legal_game(10, 999), &[]);

        assert_eq!(rules(&violations), vec![MatchRule::UnknownHero]);
        assert!(only_message(&violations).contains("999"));
    }

    #[test]
    fn a_nonexistent_hero_id_on_a_ban_is_rejected() {
        let violations = check(
            &participants(),
            &one_legal_game(10, 11),
            &[ban(999, BanType::PreBan, None)],
        );

        assert_eq!(rules(&violations), vec![MatchRule::UnknownHero]);
        assert!(only_message(&violations).contains("999"));
    }

    /// The player label is free text with no table behind it, so there is
    /// nothing to check it against -- any string, a duplicate, or none at all
    /// is legal. This is the guard against someone quietly reintroducing a
    /// `player` entity.
    #[test]
    fn player_labels_are_never_validated_arbitrary_duplicate_and_absent_all_pass() {
        let mut labelled = participants();
        labelled[0].player_label = Some("Nobody On Record".into());
        labelled[1].player_label = None;
        let violations = check(&labelled, &one_legal_game(10, 11), &[]);
        assert!(violations.is_empty(), "expected none, got {violations:?}");

        let mut same_twice = participants();
        same_twice[0].player_label = Some("Tomas Ferreira".into());
        same_twice[1].player_label = Some("Tomas Ferreira".into());
        let same_label_twice = check(&same_twice, &one_legal_game(10, 11), &[]);
        assert!(
            same_label_twice.is_empty(),
            "expected none, got {same_label_twice:?}"
        );
    }

    #[test]
    fn every_broken_rule_is_reported_not_just_the_first() {
        let violations = check(
            &participants(),
            &[on_map(game(1, vec![won(10), won(10)]), 99)],
            &[],
        );

        assert_eq!(
            rule_set(&violations),
            BTreeSet::from([
                MatchRule::MapNotInPool,
                MatchRule::DuplicateHero,
                MatchRule::NotExactlyOneWinner,
            ])
        );
    }

    #[test]
    fn violations_convert_to_the_wire_shape() {
        let violations = check(&participants(), &[], &[]);
        let wire: Vec<Violation> = violations.into_iter().map(Violation::from).collect();

        assert_eq!(
            wire,
            vec![Violation::new(
                "INVALID_GAME_COUNT",
                "At least one game is required."
            )]
        );
    }

    /// Kotlin renders a `List<Int>` into a string template with its brackets,
    /// and the message the admin reads carries them. `joinToString()`, used for
    /// the id lists, does not -- so the two renderings are deliberately
    /// different and both are contract.
    #[test]
    fn multi_game_and_multi_hero_messages_keep_the_kotlin_rendering() {
        let violations = check(
            &drafts(&[10, 11], &[10, 11]),
            &[
                on_map(game(1, vec![won(10), played(11)]), 98),
                on_map(game(2, vec![won(11), played(10)]), 99),
            ],
            &[],
        );
        assert_eq!(
            only_message(&violations),
            "Game(s) [1, 2] use a map that is not in this tournament's board pool."
        );

        let unknown = check(
            &drafts(&[10, 11, 998, 999], &[10, 11]),
            &one_legal_game(10, 11),
            &[],
        );
        assert_eq!(only_message(&unknown), "Hero(es) do not exist: 998, 999.");
    }
}
