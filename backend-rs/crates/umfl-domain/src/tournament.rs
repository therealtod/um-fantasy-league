//! A tournament, a manager's entry into one, and the roster slots the entry
//! owns.
//!
//! A direct port of `tournament/Tournament.kt` and `tournament/TournamentEntry.kt`,
//! minus their persistence attributes -- Spring Data JDBC's `@Table`/`@Id`/
//! `@MappedCollection` have no counterpart here, and the row mapping lives in
//! `umfl-server` instead. These are also not wire types: the DTOs live with
//! their feature in the server, per PORTING.md §3.
//!
//! Nothing here stores a roster's cost. It is the live sum of the slots'
//! `tournament_heroes.cost`, so the two can never disagree and an unlocked roster
//! simply re-prices when an admin retunes the pool. See AGENTS.md, "No cost
//! snapshot".

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// How heroes reach a match in this tournament.
///
/// Serializes as the Kotlin constant name; `frontend/src/api/types.ts` declares
/// it as `'BANQUEST' | 'ARSENAL'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TournamentFormat {
    /// Banquest: a fixed hero pool, drafted ahead of the event.
    Banquest,
    /// Arsenal: heroes are picked and banned match by match.
    Arsenal,
}

/// Where a tournament is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TournamentStatus {
    /// Announced, but registration has not opened yet.
    Scheduled,
    /// Managers may register and draft.
    RegistrationOpen,
    /// Matches are being played; rosters are frozen.
    Live,
    /// Finished; results are final.
    Completed,
}

impl TournamentStatus {
    /// The Kotlin constant name.
    ///
    /// `TOURNAMENT_CLOSED`'s message interpolates the status directly, and
    /// Kotlin renders an enum as its constant name -- so this string is
    /// user-visible wire text, not a debug label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "SCHEDULED",
            Self::RegistrationOpen => "REGISTRATION_OPEN",
            Self::Live => "LIVE",
            Self::Completed => "COMPLETED",
        }
    }
}

impl std::fmt::Display for TournamentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A real Unmatched tournament, and the fantasy parameters layered over it.
///
/// Dates are [`NaiveDate`], not an instant: a tournament is announced as a day
/// or a weekend, not a wall-clock moment. `end_date` is `None` until it is over.
///
/// `credit_grant` is the budget every registrant receives. It is snapshotted
/// onto the entry at registration, so retuning it later cannot disturb rosters
/// that were already drafted against the old number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tournament {
    pub id: Option<i64>,
    pub name: String,
    pub format: TournamentFormat,
    pub status: TournamentStatus,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub capacity: i32,
    pub roster_size: i32,
    pub credit_grant: i32,
}

impl Tournament {
    /// Only an open tournament takes new entries.
    pub fn accepts_registration(&self) -> bool {
        self.status == TournamentStatus::RegistrationOpen
    }

    /// Rosters freeze once the tournament goes live.
    pub fn accepts_roster_changes(&self) -> bool {
        matches!(
            self.status,
            TournamentStatus::Scheduled | TournamentStatus::RegistrationOpen
        )
    }
}

/// Whether a roster may still change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntryStatus {
    /// Roster is still being drafted and may be changed.
    Draft,
    /// Roster is committed and immutable.
    Locked,
}

/// A single hero on a roster.
///
/// Child of the [`TournamentEntry`] aggregate; its position is the list index,
/// persisted to `entry_slots.slot_index`. That index is assigned by position and
/// by nothing else -- see the plan's risk 3, "hand-assigned list indices", which
/// is why the writer must never reorder this `Vec` on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntrySlot {
    pub hero_id: i64,
}

/// One manager's entry into one tournament -- the aggregate root that owns the
/// roster slots.
///
/// `credit_grant` is copied off the tournament at registration, which is what
/// makes a granted budget stable for the manager who received it.
/// [`crate::roster_policy::validate_lock`] takes the budget from here, never
/// from the tournament.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentEntry {
    pub id: Option<i64>,
    pub tournament_id: i64,
    pub manager_id: i64,
    pub status: EntryStatus,
    pub credit_grant: i32,
    pub registered_at: DateTime<Utc>,
    pub locked_at: Option<DateTime<Utc>>,
    pub slots: Vec<EntrySlot>,
}

impl TournamentEntry {
    pub fn hero_ids(&self) -> Vec<i64> {
        self.slots.iter().map(|s| s.hero_id).collect()
    }

    pub fn is_locked(&self) -> bool {
        self.status == EntryStatus::Locked
    }

    /// Kotlin's `lock(at)`: commits the roster and stamps the moment.
    pub fn lock(&mut self, at: DateTime<Utc>) {
        self.status = EntryStatus::Locked;
        self.locked_at = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tournament(status: TournamentStatus) -> Tournament {
        Tournament {
            id: Some(2),
            name: "Winter of Champions".into(),
            format: TournamentFormat::Arsenal,
            status,
            start_date: NaiveDate::from_ymd_opt(2026, 8, 14).unwrap(),
            end_date: None,
            capacity: 64,
            roster_size: 3,
            credit_grant: 10_000,
        }
    }

    #[test]
    fn only_an_open_tournament_takes_entries() {
        assert!(tournament(TournamentStatus::RegistrationOpen).accepts_registration());
        for closed in [
            TournamentStatus::Scheduled,
            TournamentStatus::Live,
            TournamentStatus::Completed,
        ] {
            assert!(!tournament(closed).accepts_registration(), "{closed}");
        }
    }

    #[test]
    fn rosters_freeze_once_the_tournament_goes_live() {
        assert!(tournament(TournamentStatus::Scheduled).accepts_roster_changes());
        assert!(tournament(TournamentStatus::RegistrationOpen).accepts_roster_changes());
        assert!(!tournament(TournamentStatus::Live).accepts_roster_changes());
        assert!(!tournament(TournamentStatus::Completed).accepts_roster_changes());
    }

    /// `TOURNAMENT_CLOSED`'s message interpolates the status, so its rendering
    /// is wire text the frontend shows the user.
    #[test]
    fn statuses_render_as_their_kotlin_constant_names() {
        assert_eq!(
            TournamentStatus::RegistrationOpen.to_string(),
            "REGISTRATION_OPEN"
        );
        assert_eq!(TournamentStatus::Live.to_string(), "LIVE");
        assert_eq!(
            serde_json::to_string(&TournamentStatus::RegistrationOpen).unwrap(),
            r#""REGISTRATION_OPEN""#
        );
        assert_eq!(
            serde_json::to_string(&TournamentFormat::Banquest).unwrap(),
            r#""BANQUEST""#
        );
        assert_eq!(
            serde_json::to_string(&EntryStatus::Locked).unwrap(),
            r#""LOCKED""#
        );
    }

    #[test]
    fn locking_stamps_the_moment() {
        let mut entry = TournamentEntry {
            id: Some(10),
            tournament_id: 2,
            manager_id: 1,
            status: EntryStatus::Draft,
            credit_grant: 10_000,
            registered_at: "2026-07-30T09:00:00Z".parse().unwrap(),
            locked_at: None,
            slots: vec![EntrySlot { hero_id: 7 }, EntrySlot { hero_id: 9 }],
        };
        assert!(!entry.is_locked());
        assert_eq!(entry.hero_ids(), vec![7, 9]);

        let at: DateTime<Utc> = "2026-08-01T09:00:00Z".parse().unwrap();
        entry.lock(at);

        assert!(entry.is_locked());
        assert_eq!(entry.locked_at, Some(at));
    }
}
