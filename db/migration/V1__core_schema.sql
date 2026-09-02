-- ===========================================================================
-- Core schema baseline. This models a fantasy layer over REAL Unmatched
-- tournaments. A hero is only an identity (name + artwork). Everything that
-- varies -- price, legality, the board pool, the scoring weights -- hangs off
-- the tournament that decided it. There is deliberately no `season` column
-- anywhere: the tournament is the unit of scoping, and a season is just a
-- label someone puts in a tournament name.
--
-- Schema only -- no data lives in this file. Data splits in two after it, and
-- the split is by *kind*, not by environment:
--
--   * `db/migration/V2__reference_data.sql` -- the canonical heroes and boards
--     Unmatched has printed. Facts about the game, not about a league, so they
--     migrate in every profile: without them an admin has nothing to price
--     into a tournament pool.
--   * `db/seed/V3__demo_fixtures.sql` -- three demo tournaments, seeded
--     managers, a full recorded result set. A separate Flyway location that
--     only the `dev` and `test` profiles add to `spring.flyway.locations` (see
--     `application-dev.yml` / `application-test.yml`).
--
-- So a real deployment (`prod`, or any start with no profile at all) comes up
-- with a full hero and board catalogue and a league that has never been
-- played: zero tournaments, zero managers, zero results.
--
-- Naming: `game_maps` and `tournament_matches`, because MAP and MATCH are
-- reserved words in the SQL standard and quoting them at every call site
-- would be noise.
--
-- Note on surrogate keys: tables that Spring Data JDBC loads or writes as
-- aggregates carry a bigserial id, because Spring Data JDBC cannot map a
-- composite primary key. The pure link tables (`tournament_heroes`,
-- `tournament_maps`, `entry_slots`, `match_participants`, `match_game_participants`,
-- `hero_bans`, `match_hero_picks`) are read through JdbcClient or mapped as
-- @MappedCollection children keyed by list position or by their own natural
-- key, so they keep their natural composite key with no surrogate. `match_games`
-- is the one exception among the match tables: it is itself the parent of
-- `match_game_participants`, so it needs a surrogate id to be referenced by.
-- ===========================================================================

-- Names are unique because every seed insert and every integration test looks
-- a hero up by name -- there is no slug, and no stable external id to key on.
create table heroes (
    id        bigserial primary key,
    name      text not null unique,
    image_url text
);

comment on column heroes.image_url is
    'Optional artwork URL. Null for all seeded heroes -- no official Unmatched artwork is bundled.';

create table game_maps (
    id   bigserial primary key,
    name text not null unique
);

-- ===========================================================================
-- League domain: managers, tournaments, and the rosters they enter.
-- ===========================================================================

-- `auth_user_id` links a manager to a Supabase Auth identity (auth.users.id),
-- for managers who sign in via Supabase Auth (Discord OAuth in production).
-- Nullable + unique: seeded managers (NeonStrategist, etc.) have no linked
-- identity and stay usable via DevManagerProvider in dev/test; new managers
-- get just-in-time provisioned with this column set on first login.
--
-- There is no credit balance here. Budget is granted per registration
-- (`tournament_entries.credit_grant`), not held in a global wallet.
--
-- `is_admin` is our own authorization data, not derived from any identity
-- provider claim: a future swap of auth provider must not require
-- re-deriving who is allowed to run the admin API (`/api/admin/**`).
create table managers (
    id            bigserial primary key,
    handle        text not null unique,
    display_name  text not null,
    auth_user_id  uuid unique,
    is_admin      boolean not null default false
);

comment on column managers.auth_user_id is
    'Supabase auth.users.id (JWT "sub" claim). Null for dev-seeded managers with no linked identity.';

comment on column managers.is_admin is
    'Grants ROLE_ADMIN for /api/admin/**. Our own authorization data, independent of any auth provider.';

-- `credit_grant` is the fantasy budget every registrant receives on entering.
-- Dates are `date`, not `timestamptz`: a real tournament is announced as a day
-- or a weekend, not a wall-clock instant. `end_date` is null until it is over.
create table tournaments (
    id           bigserial primary key,
    name         text    not null unique,
    format       text    not null check (format in ('BANQUEST', 'ARSENAL')),
    status       text    not null check (status in ('SCHEDULED', 'REGISTRATION_OPEN', 'LIVE', 'COMPLETED')),
    start_date   date    not null,
    end_date     date,
    capacity     integer not null check (capacity > 0),
    roster_size  integer not null check (roster_size > 0),
    credit_grant integer not null check (credit_grant > 0)
);

-- The hero pool for one tournament, with its price. Cost is per tournament by
-- design: the same hero is a bargain at one event and a premium pick at the
-- next, and an admin retunes a pool with one UPDATE. Nothing snapshots it --
-- an unlocked roster simply re-prices when the cost changes.
create table tournament_heroes (
    tournament_id bigint  not null references tournaments (id) on delete cascade,
    hero_id       bigint  not null references heroes (id),
    cost          integer not null check (cost > 0),
    primary key (tournament_id, hero_id)
);

-- The legal board pool for one tournament. `tournament_matches` carries a
-- composite foreign key onto this primary key, which is what constrains a
-- recorded match to a board that tournament actually played on.
create table tournament_maps (
    tournament_id bigint not null references tournaments (id) on delete cascade,
    map_id        bigint not null references game_maps (id),
    primary key (tournament_id, map_id)
);

-- A manager's entry into a tournament. This is the aggregate root that owns the
-- roster slots below.
--
-- `credit_grant` is snapshotted from the tournament at registration: raising a
-- tournament's grant later must not silently hand extra budget to managers who
-- already drafted, and lowering it must not retroactively invalidate them.
-- Total cost is deliberately NOT stored -- it is the live sum of the slots'
-- `tournament_heroes.cost`, so it cannot drift.
create table tournament_entries (
    id            bigserial   not null primary key,
    tournament_id bigint      not null references tournaments (id) on delete cascade,
    manager_id    bigint      not null references managers (id) on delete cascade,
    status        text        not null check (status in ('DRAFT', 'LOCKED')),
    credit_grant  integer     not null check (credit_grant > 0),
    registered_at timestamptz not null default now(),
    locked_at     timestamptz,
    unique (tournament_id, manager_id),
    constraint tournament_entry_locked_has_timestamp
        check (status <> 'LOCKED' or locked_at is not null)
);

-- Child of the TournamentEntry aggregate. Spring Data JDBC maps this as a keyed
-- collection: entry_id is the back-reference, slot_index the list index.
--
-- No acquisition_cost column: the roster's cost is joined live from
-- `tournament_heroes`. The hero FK has no cascade on purpose -- deleting a hero
-- that sits on somebody's roster should fail loudly rather than quietly shrink
-- a locked entry.
create table entry_slots (
    entry_id   bigint  not null references tournament_entries (id) on delete cascade,
    slot_index integer not null check (slot_index >= 0),
    hero_id    bigint  not null references heroes (id),
    primary key (entry_id, slot_index),
    unique (entry_id, hero_id)
);

-- No index leading with tournament_id: the `unique (tournament_id, manager_id)`
-- constraint above already provides that btree. manager_id has no such
-- coverage, despite backing findByManagerId on every /api/tournaments request
-- from a signed-in manager.
create index idx_tournament_entry_manager on tournament_entries (manager_id);
create index idx_tournament_hero_hero on tournament_heroes (hero_id);
create index idx_entry_slot_hero on entry_slots (hero_id);

-- ===========================================================================
-- Recorded results and the scoring rules that price them.
--
-- Nothing in here is simulated and nothing in here stores a fantasy total.
-- Matches are facts an admin records; points are derived at read time by
-- folding `scoring_coefficients` over each (hero, match) pair. A stored total
-- would be a cache with nothing to invalidate it, because coefficients are
-- mutable reference data.
-- ===========================================================================

-- The scoring configuration for one tournament. Multiple rule sets may exist
-- (drafts, a retune kept for reference); exactly one may be active at a time,
-- enforced by the partial unique index below rather than by a trigger.
--
-- `is_active` defaults to false: which rule set is live is an explicit admin
-- decision (`POST .../scoring-rule-sets/{id}/activate`), never a property of
-- having been inserted. Spring Data JDBC writes every mapped column on
-- insert regardless, so this default only matters to a hand-written INSERT
-- (a fixture, a psql session) -- and there, defaulting to active is exactly
-- wrong: it would make a draft rule set live, or trip
-- `uq_scoring_rule_set_active` if the tournament already had one.
create table scoring_rule_sets (
    id            bigserial primary key,
    tournament_id bigint  not null references tournaments (id) on delete cascade,
    name          text    not null,
    is_active     boolean not null default false,
    unique (tournament_id, name)
);

create unique index uq_scoring_rule_set_active
    on scoring_rule_sets (tournament_id)
    where is_active;

-- One weighted metric. `metric` is deliberately free-form text, not an enum and
-- not a foreign key: an admin must be able to add a row without a migration.
-- A Kotlin registry prices the keys it implements and silently ignores the
-- rest, so an unknown metric costs nothing and breaks nothing.
--
-- The CHECK is a typo guard (SCREAMING_SNAKE only), not a whitelist -- it stops
-- 'win ' and 'Win' from becoming separate columns, without pinning the set of
-- legal metrics into the schema.
--
-- `sort_order` fixes the left-to-right column order on the leaderboard, which
-- the backend cannot know any other way.
--
-- No check on `coefficient`: negative weights are legitimate (a penalty).
create table scoring_coefficients (
    id          bigserial      primary key,
    rule_set_id bigint         not null references scoring_rule_sets (id) on delete cascade,
    metric      text           not null,
    coefficient numeric(10, 4) not null,
    sort_order  integer        not null default 0,
    unique (rule_set_id, metric),
    constraint scoring_coefficient_metric_format
        check (metric ~ '^[A-Z][A-Z0-9_]*$')
);

-- One recorded match -- a series of one or more games between the same two
-- human players. `map_id` moved to `match_games`: each game in a series can be
-- played on a different board. `played_at` stays here (when the series
-- happened), not derived from its games, so the existing played_at-based
-- ticker sort/index below is undisturbed by adding games.
--
-- Rows are seeded in chronological order, so `id` order and `played_at` order
-- agree. That is what lets the ticker poll on `id > :sinceMatchId` (monotonic,
-- unique) while sorting for display on `played_at` (which is NOT unique --
-- parallel tables share a start time).
create table tournament_matches (
    id            bigserial   primary key,
    tournament_id bigint      not null references tournaments (id) on delete cascade,
    round         integer     not null check (round > 0),
    played_at     timestamptz not null,
    external_link text        not null,
    -- Redundant against the primary key, and here only so `match_games` can
    -- point a composite FK at (id, tournament_id) -- see that table's comment.
    unique (id, tournament_id)
);

-- What stops the same match being imported twice: the admin match import
-- turns a tabletopleague.com URL into a reviewable draft, and this is the
-- duplicate check against it. Scoped to the tournament, not global, matching
-- the importer's own per-tournament check -- a global constraint would let
-- the wizard report "no duplicate" and then fail the save.
create unique index uq_tournament_match_external_link
    on tournament_matches (tournament_id, external_link);

comment on column tournament_matches.external_link is
    'Where this match is recorded on another platform (the match import''s source URL, a bracket '
    'site, a VOD). Required, and unique within the tournament: it is what stops the same match '
    'being imported twice. A match with no such page carries a synthetic ''urn:umfl:match:<id>'' '
    'placeholder rather than a null.';

-- One side of the series: which human played it, for the whole series. Hero,
-- map, health and winner all moved to match_games/match_game_participants,
-- because in a best-of-N series a side can pilot a different hero per game --
-- only the two human competitors are fixed for the series.
--
-- No surrogate id: `side` (0 or 1) is a stable ordinal with no data of its
-- own, exactly like `entry_slots.slot_index` -- Spring Data JDBC maps this as
-- a List<MatchParticipant> child keyed by list position, so the Kotlin class
-- carries no explicit `side` field.
create table match_participants (
    match_id     bigint  not null references tournament_matches (id) on delete cascade,
    side         integer not null check (side in (0, 1)),
    player_label text,
    primary key (match_id, side)
);

comment on column match_participants.player_label is
    'Who piloted this side for the whole series, as free text. Deliberately not a `player` '
    'table: every point in this application is scored per hero -- no metric extractor, no '
    'scoring coefficient and no standings query reads the human behind the hero, so the name is '
    'display text for the ticker and the admin match list and nothing more. Nullable: an '
    'unattributed result is still a valid result.';

-- One game within a series. `tournament_id` is denormalized from
-- `tournament_matches` purely so this table can carry the same composite
-- "map is in this tournament's pool" foreign key `tournament_matches` used to
-- carry directly -- Spring Data JDBC needs it as a real field to build that
-- FK; nothing besides construction reads it otherwise.
-- Being a copy, it can in principle drift from the parent match's own
-- `tournament_id`, and a game filed under the wrong tournament is invisible
-- to MapPoolAdminRepository.hasRecordedMatch (which filters on this column)
-- while its pool FK points at some other tournament's pool row -- so
-- `match_game_of_match` pins the copy to its parent instead of leaving the
-- two in step by AdminMatchService's construction alone.
-- `match_game_map_in_pool` is DEFERRABLE INITIALLY DEFERRED: deleting a
-- tournament cascades to `tournament_maps` (a direct child) and to `match_games`
-- (a grandchild, via `tournament_matches`) in the same statement, and Postgres
-- does not guarantee the grandchild is fully cascaded away before the direct
-- child's rows are removed. Deferring the check to commit, by which point
-- both cascades have finished, avoids a spurious FK violation on a delete
-- that is actually consistent. `tournament_match_map_in_pool` never needed
-- this because `tournament_matches` and `tournament_maps` are both direct
-- children of `tournaments`, one level removed either way.
create table match_games (
    id            bigserial primary key,
    match_id      bigint  not null references tournament_matches (id) on delete cascade,
    tournament_id bigint  not null,
    game_number   integer not null check (game_number > 0),
    map_id        bigint  not null,
    unique (match_id, game_number),
    constraint match_game_of_match
        foreign key (match_id, tournament_id) references tournament_matches (id, tournament_id)
        on delete cascade,
    constraint match_game_map_in_pool
        foreign key (tournament_id, map_id) references tournament_maps (tournament_id, map_id)
        deferrable initially deferred
);

-- One side's result in one game: the hero it brought and how it ended.
--
-- A losing hero has 0 or less health at the end of the game. The winner can
-- have any health, including a negative value. Exactly one side of a game
-- carries `is_winner` -- a game always has a winner, enforced in
-- MatchResultPolicy (NOT_EXACTLY_ONE_WINNER) rather than here, since a partial
-- unique index could pin "at most one" but not "at least one".
--
-- `unique (game_id, hero_id)` is unambiguous here precisely because banned
-- heroes live in `hero_bans` and never appear as a game participant.
--
-- `side` here is NOT declared as an FK back to match_participants.side --
-- doing that would require denormalizing match_id onto this table too, one
-- level further down. The side-pairing between match_participants and
-- match_game_participants (side 0 of a game is played by side 0 of the
-- series) is an APPLICATION-level invariant, enforced by MatchResultPolicy /
-- AdminMatchService always building both lists in the same order -- the same
-- tier of guarantee "exactly 2 participants" already was before this change.
create table match_game_participants (
    game_id          bigint  not null references match_games (id) on delete cascade,
    side             integer not null check (side in (0, 1)),
    hero_id          bigint  not null references heroes (id),
    health_remaining integer not null,
    is_winner        boolean not null default false,
    primary key (game_id, side),
    unique (game_id, hero_id),
    constraint match_game_participant_loser_defeated check (is_winner or health_remaining <= 0)
);

create unique index uq_match_game_participant_winner
    on match_game_participants (game_id)
    where is_winner;

-- A hero banned out of the series. Modelled as its own table rather than a
-- `was_banned` flag on match_game_participants: a banned hero has no health
-- and no result, so storing it as a participant would force
-- health_remaining = 0, which is indistinguishable from "played and was
-- defeated" and would poison every HEALTH_REMAINING sum and SHUTOUT check.
--
-- Bans happen once, before the series starts -- not per game -- which is why
-- this references tournament_matches, not match_games. `ban_type` follows this
-- file's existing check-constrained text + Kotlin enum convention (see
-- tournaments.status).
--
-- The primary key stays (match_id, hero_id): a hero is banned at most once per
-- series however many sides wanted it, which is exactly what
-- MatchRule.DUPLICATE_BAN already means.
--
-- `side` is the side whose draft the hero was struck out of, not who struck
-- it -- `ban_type` already says that: SELF_BAN is a side striking one of its
-- own, OPPONENT_BAN the other side striking it, and a PRE_BAN precedes side
-- assignment and so carries no side at all (`hero_ban_pre_ban_has_no_side`
-- rejects one that does). It stays nullable rather than becoming a
-- non-optional fact: a ban typed by hand with nothing to attribute it to is
-- still a legal ban. Scoring never reads this column -- MatchMetrics.banOfType
-- prices a ban by category alone, because points are per hero and never per
-- player.
--
-- A parallel `map_ban (match_id, map_id, ban_type)` table is a plausible
-- future extension for board draft/bans -- not built now.
create table hero_bans (
    match_id bigint  not null references tournament_matches (id) on delete cascade,
    hero_id  bigint  not null references heroes (id),
    ban_type text    not null check (ban_type in ('PRE_BAN', 'OPPONENT_BAN', 'SELF_BAN')),
    side     integer,
    primary key (match_id, hero_id),
    constraint hero_ban_side_range check (side in (0, 1)),
    constraint hero_ban_pre_ban_has_no_side check (side is null or ban_type <> 'PRE_BAN')
);

comment on column hero_bans.side is
    'The side whose draft this hero was struck out of (0 or 1), or null for a PRE_BAN -- '
    'struck before sides were assigned -- or for a ban typed with no side to attribute. '
    'ban_type says who cast the ban; this says whose hero it was.';

-- A hero one side drafted for one series -- the picks half of the draft, with
-- hero_bans as the bans half. A drafted hero need never have played: that is
-- the whole point of recording it, and it is what APPEARANCE scores. Without
-- this table, "featured in this match" and "played a game in this match" were
-- the same fact, which made a hero drafted and then never fielded
-- indistinguishable from a hero nobody drafted at all.
--
-- A separate table from `hero_bans` rather than one draft table with a kind
-- column, because the two carry different data: a pick has an owning `side`
-- and no category, a ban has a category (`PRE_BAN`/`OPPONENT_BAN`/`SELF_BAN`)
-- and no side -- a pre-ban belongs to neither. A hero cannot be both; that is
-- enforced in Kotlin as `MatchRule.BANNED_HERO_DRAFTED`, alongside
-- `PLAYED_HERO_NOT_DRAFTED`, which is what keeps a recorded draft complete.
--
-- Deliberately no `unique (match_id, hero_id)`. Games in a series are
-- independent -- side 0 may pilot a hero in game 1 and side 1 the same hero in
-- game 2, which `match_game_participants`'s per-game `unique (game_id,
-- hero_id)` allows -- so a cross-side unique here would retroactively outlaw a
-- result the schema accepts today. APPEARANCE de-duplicates by hero instead,
-- so a hero drafted by both sides still scores once for the match.
create table match_hero_picks (
    match_id bigint  not null references tournament_matches (id) on delete cascade,
    side     integer not null check (side in (0, 1)),
    hero_id  bigint  not null references heroes (id),
    primary key (match_id, side, hero_id)
);

create index idx_scoring_coefficient_rule_set on scoring_coefficients (rule_set_id, sort_order);
create index idx_tournament_match_tournament on tournament_matches (tournament_id, id desc);
create index idx_tournament_match_played_at on tournament_matches (tournament_id, played_at desc);
create index idx_match_game_participant_hero on match_game_participants (hero_id);
create index idx_hero_ban_hero on hero_bans (hero_id);
create index idx_match_hero_pick_hero on match_hero_picks (hero_id);
