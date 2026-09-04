//! A recorded match as the read side assembles it, and the three shapes a hero
//! can have inside one.
//!
//! Plus the `MetricContext`/`HeroRole` pair that would otherwise sit beside
//! the metric registry in `match_metrics.rs` -- they live here instead
//! because [`MatchResult::hero_contexts`] is the only thing that constructs
//! one, and a context that outlives its match is not representable.
//!
//! These are deliberately plain records with no persistence attributes: reads
//! never go through the write aggregate. Nothing here stores a point total --
//! standings, ticker and points are all derived from these at read time.

use crate::time::java_instant;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Why a hero was struck out of a series.
///
/// Serializes as the constant name below, which is what
/// `frontend/src/api/types.ts` matches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BanType {
    /// Struck before sides were assigned, so it belongs to neither draft and
    /// scores neither ban metric.
    PreBan,
    /// The other side struck it.
    OpponentBan,
    /// A side struck one of its own.
    SelfBan,
}

impl BanType {
    /// `match_policy::validate`'s `BAN_SIDE_INVALID` message renders the ban
    /// type into user-visible text, which is why this spelling is contract
    /// rather than a debug convenience.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreBan => "PRE_BAN",
            Self::OpponentBan => "OPPONENT_BAN",
            Self::SelfBan => "SELF_BAN",
        }
    }
}

impl std::fmt::Display for BanType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One hero on one side's draft board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftedHeroResult {
    pub hero_id: i64,
    pub hero_name: String,
}

/// One human's side of a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchParticipantResult {
    /// 0 or 1 -- a stable ordinal for the whole series, matching
    /// `match_participants.side`.
    pub side: i32,
    /// Who piloted this side, as free text. `None` when recorded unattributed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_label: Option<String>,
    /// The heroes this side drafted for the series, whether or not it fielded
    /// them. Every hero that played one of the games is in here -- a recorded
    /// draft is complete (`MatchRule::PLAYED_HERO_NOT_DRAFTED`) -- so the ones
    /// that appear in no game are the picks that never hit the table.
    #[serde(default)]
    pub drafted_heroes: Vec<DraftedHeroResult>,
}

/// One hero's result in one game of a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameParticipantResult {
    pub side: i32,
    pub hero_id: i64,
    pub hero_name: String,
    /// The hero's health at the end of this game. 0 means defeated, and a
    /// losing side may finish below it -- an overkill hit lands it negative.
    pub health_remaining: i32,
    pub is_winner: bool,
}

/// One game of a series: its own map, its own two participants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResult {
    pub game_id: i64,
    pub game_number: i32,
    pub map_id: i64,
    pub map_name: String,
    pub participants: Vec<GameParticipantResult>,
}

impl GameResult {
    /// The side that took this game. Never `None` for a recorded game:
    /// `MatchResultPolicy` rejects a submission whose game has anything other
    /// than exactly one winner, so there is no drawn game to represent.
    ///
    /// The wire response also carries an undeclared `winner` field derived
    /// from this; that's added on the server's DTO, not here -- a domain type
    /// has no business carrying a field only the wire wants.
    pub fn winner(&self) -> Option<&GameParticipantResult> {
        self.participants.iter().find(|p| p.is_winner)
    }

    /// Everyone in this game who is not the named hero. `HEALTH_DIFFERENTIAL`
    /// needs it, which is why a bare participant row is not enough context.
    pub fn opponents_of(&self, hero_id: i64) -> Vec<&GameParticipantResult> {
        self.participants
            .iter()
            .filter(|p| p.hero_id != hero_id)
            .collect()
    }
}

/// A hero banned out of the series, categorized by when/why it was struck.
///
/// `side` is whose draft it came out of; `ban_type` is who struck it. `None`
/// for a `PRE_BAN` -- struck before sides were assigned, so it belongs to
/// neither -- and for anything recorded before `hero_bans.side` existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanResult {
    pub hero_id: i64,
    pub hero_name: String,
    pub ban_type: BanType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<i32>,
}

/// A recorded match, as read back out of the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchResult {
    pub match_id: i64,
    pub tournament_id: i64,
    pub round: i32,
    #[serde(with = "java_instant")]
    pub played_at: DateTime<Utc>,
    pub external_link: String,
    pub participants: Vec<MatchParticipantResult>,
    /// Ordered by `GameResult::game_number`.
    pub games: Vec<GameResult>,
    pub bans: Vec<BanResult>,
}

impl MatchResult {
    pub fn player_label_for_side(&self, side: i32) -> Option<&str> {
        self.participants
            .iter()
            .find(|p| p.side == side)
            .and_then(|p| p.player_label.as_deref())
    }

    /// Every hero either side drafted, once for the series however many sides
    /// took it. There is deliberately no `unique (match_id, hero_id)` on
    /// `match_hero_picks` -- games are independent, so a hero may go to one side
    /// in game 1 and the other in game 2 -- so the de-duplication happens here.
    ///
    /// First-encounter order.
    pub fn drafted_hero_ids(&self) -> Vec<i64> {
        let mut seen = Vec::new();
        for participant in &self.participants {
            for hero in &participant.drafted_heroes {
                if !seen.contains(&hero.hero_id) {
                    seen.push(hero.hero_id);
                }
            }
        }
        seen
    }

    /// The match's contexts, in the three shapes a hero can have here.
    ///
    /// [`HeroRole::Played`] is per game: every (hero, game) this match's games
    /// touched, so a hero played in two games of a Bo3 yields two contexts,
    /// each scoring independently. [`HeroRole::Drafted`] and
    /// [`HeroRole::Banned`] are both per *series*, exactly once each, because
    /// the draft happens once before any game is played and must not be
    /// multiplied by game count -- which is why a hero that plays all three
    /// games of a Bo3 still appears once. That split is the whole reason
    /// `APPEARANCE` is not silently scaled by series length.
    ///
    /// A drafted hero that played therefore yields both: N `Played` contexts
    /// for the per-game metrics, plus the one `Drafted` context `APPEARANCE`
    /// prices.
    ///
    /// A hero can be none of drafted, played or banned in the same match --
    /// `match_policy::validate` rejects those submissions as
    /// `BANNED_HERO_DRAFTED` and `BANNED_HERO_PLAYED` -- so the exclusions
    /// below are a backstop for
    /// data that predates the checks, not supported input. Precedence is
    /// **Played > Banned > Drafted**: playing wins over a ban, because it has
    /// real health and a real result attached, and a hero that played is
    /// credited with the draft that fielded it.
    pub fn hero_contexts(&self) -> Vec<MetricContext<'_>> {
        let played: Vec<MetricContext<'_>> = self
            .games
            .iter()
            .flat_map(|game| {
                game.participants
                    .iter()
                    .map(move |participant| MetricContext {
                        match_result: self,
                        hero_id: participant.hero_id,
                        role: HeroRole::Played { game, participant },
                    })
            })
            .collect();
        let played_hero_ids: Vec<i64> = played.iter().map(|c| c.hero_id).collect();

        let mut banned_hero_ids: Vec<i64> = Vec::new();
        let banned: Vec<MetricContext<'_>> = self
            .bans
            .iter()
            .filter(|ban| !played_hero_ids.contains(&ban.hero_id))
            // First ban wins, however many rows name the hero.
            .filter(|ban| {
                let fresh = !banned_hero_ids.contains(&ban.hero_id);
                if fresh {
                    banned_hero_ids.push(ban.hero_id);
                }
                fresh
            })
            .map(|ban| MetricContext {
                match_result: self,
                hero_id: ban.hero_id,
                role: HeroRole::Banned,
            })
            .collect();

        let drafted = self
            .drafted_hero_ids()
            .into_iter()
            .filter(|hero_id| !banned_hero_ids.contains(hero_id))
            .map(|hero_id| MetricContext {
                match_result: self,
                hero_id,
                role: HeroRole::Drafted,
            });

        // played + drafted + banned, in that order. Nothing downstream sorts
        // these, and `StandingsService.ticker` banks a hero's Drafted context
        // against its first game, so the order is worth keeping.
        played.into_iter().chain(drafted).chain(banned).collect()
    }
}

/// What a hero did in one match.
///
/// `Played` is per *game*: there is a game + participant row, and a hero that
/// played three games of a Bo3 has three of these. `Drafted` and `Banned` are
/// per *series* and mutually exclusive -- a hero was taken in the match's draft
/// or struck out of it, once, before any game was played. A hero that was
/// drafted and then fielded has both a `Drafted` role and its `Played` ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroRole<'a> {
    Played {
        game: &'a GameResult,
        participant: &'a GameParticipantResult,
    },
    Drafted,
    Banned,
}

/// One hero's role in one match, with the whole match still visible.
///
/// The extractors need more than a participant row: `HEALTH_DIFFERENTIAL` needs
/// the opponent, and `APPEARANCE`/`SELF_BAN`/`OPPONENT_BAN` have no participant
/// row at all -- they price the draft, reading the match's bans or the role
/// itself. `WIN` and `LOSS` are scoped per game rather than per series: a hero
/// that takes game 1 and drops game 2 scores one of each.
///
/// The first field is named `match_result` rather than `match`, which is a
/// reserved keyword in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricContext<'a> {
    pub match_result: &'a MatchResult,
    pub hero_id: i64,
    pub role: HeroRole<'a>,
}

impl<'a> MetricContext<'a> {
    /// The participant row, when this context is a played game.
    pub fn participant(&self) -> Option<&'a GameParticipantResult> {
        match self.role {
            HeroRole::Played { participant, .. } => Some(participant),
            _ => None,
        }
    }

    /// Everyone else in the same game. Empty for a `Drafted` or `Banned`
    /// context, which has no game to look at.
    pub fn opponents(&self) -> Vec<&'a GameParticipantResult> {
        match self.role {
            HeroRole::Played { game, .. } => game.opponents_of(self.hero_id),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn hero(id: i64) -> DraftedHeroResult {
        DraftedHeroResult {
            hero_id: id,
            hero_name: format!("Hero {id}"),
        }
    }

    fn side(side: i32, drafted: &[i64]) -> MatchParticipantResult {
        MatchParticipantResult {
            side,
            player_label: Some(format!("Player {side}")),
            drafted_heroes: drafted.iter().copied().map(hero).collect(),
        }
    }

    /// `(hero_id, health_remaining, is_winner)` per side, in side order.
    fn game(number: i32, rows: [(i64, i32, bool); 2]) -> GameResult {
        GameResult {
            game_id: i64::from(number) + 100,
            game_number: number,
            map_id: 7,
            map_name: "Marmoreal".to_string(),
            participants: rows
                .iter()
                .enumerate()
                .map(|(index, &(hero_id, health, won))| GameParticipantResult {
                    side: index as i32,
                    hero_id,
                    hero_name: format!("Hero {hero_id}"),
                    health_remaining: health,
                    is_winner: won,
                })
                .collect(),
        }
    }

    fn ban(hero_id: i64, ban_type: BanType, on_side: Option<i32>) -> BanResult {
        BanResult {
            hero_id,
            hero_name: format!("Hero {hero_id}"),
            ban_type,
            side: on_side,
        }
    }

    fn match_of(
        participants: Vec<MatchParticipantResult>,
        games: Vec<GameResult>,
        bans: Vec<BanResult>,
    ) -> MatchResult {
        MatchResult {
            match_id: 1,
            tournament_id: 1,
            round: 1,
            played_at: Utc.timestamp_opt(1_780_684_200, 0).single().expect("valid"),
            external_link: "https://tabletopleague.com/match/1".to_string(),
            participants,
            games,
            bans,
        }
    }

    fn roles_of<'a>(contexts: &'a [MetricContext<'a>], hero_id: i64) -> Vec<HeroRole<'a>> {
        contexts
            .iter()
            .filter(|c| c.hero_id == hero_id)
            .map(|c| c.role)
            .collect()
    }

    fn count_played(contexts: &[MetricContext<'_>], hero_id: i64) -> usize {
        roles_of(contexts, hero_id)
            .iter()
            .filter(|r| matches!(r, HeroRole::Played { .. }))
            .count()
    }

    /// A Bo3 where hero 1 plays all three games. It must collect three `Played`
    /// contexts and exactly *one* `Drafted` -- APPEARANCE prices the draft, and
    /// a draft happens once per series however long the series runs.
    #[test]
    fn a_hero_that_played_yields_one_played_per_game_and_one_drafted() {
        let m = match_of(
            vec![side(0, &[1, 2]), side(1, &[3])],
            vec![
                game(1, [(1, 5, true), (3, 0, false)]),
                game(2, [(1, 0, false), (3, 2, true)]),
                game(3, [(1, 8, true), (3, 0, false)]),
            ],
            vec![],
        );

        let contexts = m.hero_contexts();
        assert_eq!(count_played(&contexts, 1), 3);
        assert_eq!(
            roles_of(&contexts, 1)
                .iter()
                .filter(|r| matches!(r, HeroRole::Drafted))
                .count(),
            1,
        );
        assert_eq!(roles_of(&contexts, 1).len(), 4);
    }

    /// Hero 2 was taken and never fielded: the pick still scores an appearance,
    /// and nothing else.
    #[test]
    fn a_hero_drafted_and_never_fielded_yields_only_drafted() {
        let m = match_of(
            vec![side(0, &[1, 2]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![],
        );

        assert_eq!(roles_of(&m.hero_contexts(), 2), vec![HeroRole::Drafted]);
    }

    #[test]
    fn a_banned_hero_yields_only_banned() {
        let m = match_of(
            vec![side(0, &[1]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![ban(9, BanType::PreBan, None)],
        );

        assert_eq!(roles_of(&m.hero_contexts(), 9), vec![HeroRole::Banned]);
    }

    /// The backstop for data recorded before `BANNED_HERO_PLAYED` existed:
    /// playing wins, because it has real health and a real result attached, and
    /// the hero keeps the Drafted context that fielded it.
    #[test]
    fn played_beats_banned() {
        let m = match_of(
            vec![side(0, &[1]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![ban(1, BanType::SelfBan, Some(0))],
        );

        let contexts = m.hero_contexts();
        let roles = roles_of(&contexts, 1);
        assert!(matches!(roles[0], HeroRole::Played { .. }));
        assert_eq!(roles[1], HeroRole::Drafted);
        assert_eq!(roles.len(), 2);
    }

    /// And the `BANNED_HERO_DRAFTED` backstop: a hero on both lists is banned,
    /// not drafted, so it never scores APPEARANCE on top of its ban.
    #[test]
    fn banned_beats_drafted() {
        let m = match_of(
            vec![side(0, &[1, 4]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![ban(4, BanType::OpponentBan, Some(0))],
        );

        assert_eq!(roles_of(&m.hero_contexts(), 4), vec![HeroRole::Banned]);
    }

    /// `hero_bans` is keyed `(match_id, hero_id)`, so this cannot arrive from
    /// the database -- but dropping the de-duplication above would double a
    /// ban's points if it ever did.
    #[test]
    fn a_hero_banned_twice_yields_one_banned_context() {
        let m = match_of(
            vec![side(0, &[1]), side(1, &[3])],
            vec![],
            vec![
                ban(9, BanType::SelfBan, Some(0)),
                ban(9, BanType::OpponentBan, Some(1)),
            ],
        );

        assert_eq!(roles_of(&m.hero_contexts(), 9), vec![HeroRole::Banned]);
    }

    /// A hero taken by both sides is one series-level draft, not two.
    #[test]
    fn drafted_hero_ids_de_duplicates_across_sides_keeping_first_encounter_order() {
        let m = match_of(vec![side(0, &[5, 2]), side(1, &[2, 8])], vec![], vec![]);

        assert_eq!(m.drafted_hero_ids(), vec![5, 2, 8]);
    }

    #[test]
    fn contexts_come_back_played_then_drafted_then_banned() {
        let m = match_of(
            vec![side(0, &[1, 2]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![ban(9, BanType::PreBan, None)],
        );

        let kinds: Vec<&str> = m
            .hero_contexts()
            .iter()
            .map(|c| match c.role {
                HeroRole::Played { .. } => "played",
                HeroRole::Drafted => "drafted",
                HeroRole::Banned => "banned",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "played", "played", "drafted", "drafted", "drafted", "banned"
            ],
        );
    }

    #[test]
    fn opponents_and_participant_are_empty_off_a_played_game() {
        let m = match_of(
            vec![side(0, &[1, 2]), side(1, &[3])],
            vec![game(1, [(1, 5, true), (3, 0, false)])],
            vec![],
        );
        let contexts = m.hero_contexts();

        let played = contexts
            .iter()
            .find(|c| c.hero_id == 1 && matches!(c.role, HeroRole::Played { .. }))
            .expect("hero 1 played");
        assert_eq!(
            played.participant().expect("participant").health_remaining,
            5
        );
        assert_eq!(
            played
                .opponents()
                .iter()
                .map(|o| o.hero_id)
                .collect::<Vec<_>>(),
            vec![3],
        );

        let drafted = contexts
            .iter()
            .find(|c| c.hero_id == 2)
            .expect("hero 2 drafted");
        assert!(drafted.participant().is_none());
        assert!(drafted.opponents().is_empty());
    }

    #[test]
    fn winner_is_the_flagged_participant() {
        let g = game(1, [(1, 5, true), (3, 0, false)]);
        assert_eq!(g.winner().expect("a winner").hero_id, 1);
    }

    #[test]
    fn player_label_for_side_reads_the_series_row() {
        let m = match_of(vec![side(0, &[1]), side(1, &[3])], vec![], vec![]);
        assert_eq!(m.player_label_for_side(1), Some("Player 1"));
        assert_eq!(m.player_label_for_side(7), None);
    }

    /// An unattributed side and a sideless ban must serialize as *absent*
    /// fields, never as a JSON `null`.
    #[test]
    fn absent_optionals_are_omitted_not_null() {
        let m = match_of(
            vec![MatchParticipantResult {
                side: 0,
                player_label: None,
                drafted_heroes: vec![],
            }],
            vec![],
            vec![ban(9, BanType::PreBan, None)],
        );

        let json = serde_json::to_string(&m).expect("serialize");
        assert!(!json.contains("playerLabel"), "{json}");
        assert!(!json.contains("null"), "{json}");
        assert!(json.contains(r#""banType":"PRE_BAN""#), "{json}");
        assert!(
            json.contains(r#""playedAt":"2026-06-05T18:30:00Z""#),
            "{json}"
        );
    }
}
