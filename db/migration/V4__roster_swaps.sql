-- ===========================================================================
-- Between-round roster swaps.
--
-- A manager drafts once and locks once, and until now that was the end of it.
-- This adds the standard fantasy-league mechanic on top: between rounds an
-- admin opens a window, and each manager may exchange a limited number of
-- heroes as long as the new roster still fits the grant on their entry.
--
-- Two facts have to become storable for that to work: which round the
-- tournament is in, and which heroes each entry has traded. Everything else --
-- how many swaps a manager has left, and which heroes they held during any
-- given round -- is derived from those two, in keeping with the rest of this
-- schema. See `roster_swaps` below.
-- ===========================================================================

-- `current_round` is the round the tournament is *in*, which is not the same
-- thing as the standings board's `currentRound` -- that one is
-- max(tournament_matches.round), i.e. the latest round with a recorded result.
-- The two answer different questions and are deliberately kept apart: an admin
-- advances into a round before any of its matches exist.
--
-- `swap_window_open` is not coupled to advancing. Advancing the round and
-- opening the window are two separate admin decisions, because the window has
-- to be shut again before that round's results are recorded -- otherwise a
-- manager could read the ticker and then buy the heroes that just scored.
--
-- `swaps_per_round` at 0 means the feature is simply off for that tournament,
-- which is what every existing row gets.
alter table tournaments
    add column current_round    integer not null default 1     check (current_round > 0),
    add column swaps_per_round  integer not null default 0     check (swaps_per_round >= 0),
    add column swap_window_open boolean not null default false;

-- One hero exchanged for another, in one round, by one entry. A submission that
-- swaps two heroes writes two rows sharing a round.
--
-- This is an event log, in the same category as a recorded match: it says what
-- a manager did, never what they are owed. Both numbers the feature needs come
-- out of it rather than being maintained alongside it --
--
--   * the allowance is `swaps_per_round * (current_round - 1) - count(*)`, so
--     an unused window is banked by simply never having spent; and
--   * the roster an entry held during round N is the current `entry_slots`
--     with every row here of a later round undone.
--
-- That second one is why the log cannot be trimmed or summarised: it is what
-- makes a hero's points stay in the round it earned them, instead of following
-- the hero to whoever owns it now.
--
-- The hero FKs have no cascade, matching `entry_slots.hero_id`: deleting a hero
-- that appears in somebody's roster history should fail loudly rather than
-- quietly rewrite a past round.
create table roster_swaps (
    id          bigserial   primary key,
    entry_id    bigint      not null references tournament_entries (id) on delete cascade,
    round       integer     not null check (round > 0),
    hero_out_id bigint      not null references heroes (id),
    hero_in_id  bigint      not null references heroes (id),
    swapped_at  timestamptz not null default now(),
    constraint roster_swap_exchanges_two_heroes check (hero_out_id <> hero_in_id)
);

-- Covers both reads: the per-entry replay (ordered by round, then id, because
-- two swaps in one round still have to be undone in the order they were made)
-- and the `count(*)` behind the allowance.
create index idx_roster_swap_entry on roster_swaps (entry_id, round, id);
