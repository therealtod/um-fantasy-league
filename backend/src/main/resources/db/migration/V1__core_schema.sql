-- ===========================================================================
-- Core schema, seed fixtures, and admin authorization -- squashed into one
-- baseline. This models a fantasy layer over REAL Unmatched tournaments. A
-- hero is only an identity (name + artwork). Everything that varies -- price,
-- legality, the board pool, the scoring weights -- hangs off the tournament
-- that decided it. There is deliberately no `season` column anywhere: the
-- tournament is the unit of scoping, and a season is just a label someone
-- puts in a tournament name.
--
-- Naming: `game_map` and `tournament_match`, because MAP and MATCH are
-- reserved words in the SQL standard and quoting them at every call site
-- would be noise.
--
-- Note on surrogate keys: tables that Spring Data JDBC loads or writes as
-- aggregates carry a bigserial id, because Spring Data JDBC cannot map a
-- composite primary key. The pure link tables (`tournament_hero`,
-- `tournament_map`, `entry_slot`, `match_participant`, `match_game_participant`,
-- `hero_ban`) are read through JdbcClient or mapped as @MappedCollection
-- children keyed by list position or by their own natural key, so they keep
-- their natural composite key with no surrogate. `match_game` is the one
-- exception among the match tables: it is itself the parent of
-- `match_game_participant`, so it needs a surrogate id to be referenced by.
--
-- The seed data below is a fixture: three tournaments in three lifecycle
-- states, one of them with a complete recorded result set. Hero and map names
-- reference Unmatched (Restoration Games). No official artwork is
-- bundled, so `heroes.image_url` stays null and the UI falls back to a
-- generated placeholder. Reference rows are joined by their natural key
-- (heroes.name, game_map.name, manager.handle, tournament.name)
-- rather than by assumed serial ids, which is why those columns are unique.
-- The one exception is `tournament_match`, which has no natural key -- see
-- the comment there. Integration tests assert on these seed numbers exactly.
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

create table game_map (
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
-- (`tournament_entry.credit_grant`), not held in a global wallet.
--
-- `is_admin` is our own authorization data, not derived from any identity
-- provider claim: a future swap of auth provider must not require
-- re-deriving who is allowed to run the admin API (`/api/admin/**`).
create table manager (
    id            bigserial primary key,
    handle        text not null unique,
    display_name  text not null,
    rank_tier     text not null check (rank_tier in ('BRONZE', 'SILVER', 'GOLD', 'ELITE', 'LEGEND')),
    rank_division text not null check (rank_division in ('I', 'II', 'III')),
    auth_user_id  uuid unique,
    is_admin      boolean not null default false
);

comment on column manager.auth_user_id is
    'Supabase auth.users.id (JWT "sub" claim). Null for dev-seeded managers with no linked identity.';

comment on column manager.is_admin is
    'Grants ROLE_ADMIN for /api/admin/**. Our own authorization data, independent of any auth provider.';

-- `credit_grant` is the fantasy budget every registrant receives on entering.
-- Dates are `date`, not `timestamptz`: a real tournament is announced as a day
-- or a weekend, not a wall-clock instant. `end_date` is null until it is over.
create table tournament (
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
create table tournament_hero (
    tournament_id bigint  not null references tournament (id) on delete cascade,
    hero_id       bigint  not null references heroes (id),
    cost          integer not null check (cost > 0),
    primary key (tournament_id, hero_id)
);

-- The legal board pool for one tournament. `tournament_match` carries a
-- composite foreign key onto this primary key, which is what constrains a
-- recorded match to a board that tournament actually played on.
create table tournament_map (
    tournament_id bigint not null references tournament (id) on delete cascade,
    map_id        bigint not null references game_map (id),
    primary key (tournament_id, map_id)
);

-- A manager's entry into a tournament. This is the aggregate root that owns the
-- roster slots below.
--
-- `credit_grant` is snapshotted from the tournament at registration: raising a
-- tournament's grant later must not silently hand extra budget to managers who
-- already drafted, and lowering it must not retroactively invalidate them.
-- Total cost is deliberately NOT stored -- it is the live sum of the slots'
-- `tournament_hero.cost`, so it cannot drift.
create table tournament_entry (
    id            bigserial   not null primary key,
    tournament_id bigint      not null references tournament (id) on delete cascade,
    manager_id    bigint      not null references manager (id) on delete cascade,
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
-- `tournament_hero`. The hero FK has no cascade on purpose -- deleting a hero
-- that sits on somebody's roster should fail loudly rather than quietly shrink
-- a locked entry.
create table entry_slot (
    entry_id   bigint  not null references tournament_entry (id) on delete cascade,
    slot_index integer not null check (slot_index >= 0),
    hero_id    bigint  not null references heroes (id),
    primary key (entry_id, slot_index),
    unique (entry_id, hero_id)
);

-- No index leading with tournament_id: the `unique (tournament_id, manager_id)`
-- constraint above already provides that btree. manager_id has no such
-- coverage, despite backing findByManagerId on every /api/tournaments request
-- from a signed-in manager.
create index idx_tournament_entry_manager on tournament_entry (manager_id);
create index idx_tournament_hero_hero on tournament_hero (hero_id);
create index idx_entry_slot_hero on entry_slot (hero_id);

-- ===========================================================================
-- Recorded results and the scoring rules that price them.
--
-- Nothing in here is simulated and nothing in here stores a fantasy total.
-- Matches are facts an admin records; points are derived at read time by
-- folding `scoring_coefficient` over each (hero, match) pair. A stored total
-- would be a cache with nothing to invalidate it, because coefficients are
-- mutable reference data.
-- ===========================================================================

-- The scoring configuration for one tournament. Multiple rule sets may exist
-- (drafts, a retune kept for reference); exactly one may be active at a time,
-- enforced by the partial unique index below rather than by a trigger.
create table scoring_rule_set (
    id            bigserial primary key,
    tournament_id bigint  not null references tournament (id) on delete cascade,
    name          text    not null,
    is_active     boolean not null default true,
    unique (tournament_id, name)
);

create unique index uq_scoring_rule_set_active
    on scoring_rule_set (tournament_id)
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
create table scoring_coefficient (
    id          bigserial      primary key,
    rule_set_id bigint         not null references scoring_rule_set (id) on delete cascade,
    metric      text           not null,
    coefficient numeric(10, 4) not null,
    sort_order  integer        not null default 0,
    unique (rule_set_id, metric),
    constraint scoring_coefficient_metric_format
        check (metric ~ '^[A-Z][A-Z0-9_]*$')
);

-- One recorded match -- a series of one or more games between the same two
-- human players. `map_id` moved to `match_game`: each game in a series can be
-- played on a different board. `played_at` stays here (when the series
-- happened), not derived from its games, so the existing played_at-based
-- ticker sort/index below is undisturbed by adding games.
--
-- Rows are seeded in chronological order, so `id` order and `played_at` order
-- agree. That is what lets the ticker poll on `id > :sinceMatchId` (monotonic,
-- unique) while sorting for display on `played_at` (which is NOT unique --
-- parallel tables share a start time).
create table tournament_match (
    id            bigserial   primary key,
    tournament_id bigint      not null references tournament (id) on delete cascade,
    round         integer     not null check (round > 0),
    played_at     timestamptz not null,
    external_link text,
    -- Redundant against the primary key, and here only so `match_game` can
    -- point a composite FK at (id, tournament_id) -- see that table's comment.
    unique (id, tournament_id)
);

comment on column tournament_match.external_link is
    'Optional link to another platform''s record of this match (bracket site, VOD, etc). Same '
    'shape as heroes.image_url: plain nullable text, no validation beyond nullability.';

-- One side of the series: which human played it, for the whole series. Hero,
-- map, health and winner all moved to match_game/match_game_participant,
-- because in a best-of-N series a side can pilot a different hero per game --
-- only the two human competitors are fixed for the series.
--
-- No surrogate id: `side` (0 or 1) is a stable ordinal with no data of its
-- own, exactly like `entry_slot.slot_index` -- Spring Data JDBC maps this as
-- a List<MatchParticipant> child keyed by list position, so the Kotlin class
-- carries no explicit `side` field.
create table match_participant (
    match_id     bigint  not null references tournament_match (id) on delete cascade,
    side         integer not null check (side in (0, 1)),
    player_label text,
    primary key (match_id, side)
);

comment on column match_participant.player_label is
    'Who piloted this side for the whole series, as free text. Deliberately not a `player` '
    'table: every point in this application is scored per hero -- no metric extractor, no '
    'scoring coefficient and no standings query reads the human behind the hero, so the name is '
    'display text for the ticker and the admin match list and nothing more. Nullable: an '
    'unattributed result is still a valid result.';

-- One game within a series. `tournament_id` is denormalized from
-- `tournament_match` purely so this table can carry the same composite
-- "map is in this tournament's pool" foreign key `tournament_match` used to
-- carry directly -- Spring Data JDBC needs it as a real field to build that
-- FK; nothing besides construction reads it otherwise.
-- Being a copy, it can in principle drift from the parent match's own
-- `tournament_id`, and a game filed under the wrong tournament is invisible
-- to MapPoolAdminRepository.hasRecordedMatch (which filters on this column)
-- while its pool FK points at some other tournament's pool row -- so
-- `match_game_of_match` pins the copy to its parent instead of leaving the
-- two in step by AdminMatchService's construction alone.
-- `match_game_map_in_pool` is DEFERRABLE INITIALLY DEFERRED: deleting a
-- tournament cascades to `tournament_map` (a direct child) and to `match_game`
-- (a grandchild, via `tournament_match`) in the same statement, and Postgres
-- does not guarantee the grandchild is fully cascaded away before the direct
-- child's rows are removed. Deferring the check to commit, by which point
-- both cascades have finished, avoids a spurious FK violation on a delete
-- that is actually consistent. `tournament_match_map_in_pool` never needed
-- this because `tournament_match` and `tournament_map` are both direct
-- children of `tournament`, one level removed either way.
create table match_game (
    id            bigserial primary key,
    match_id      bigint  not null references tournament_match (id) on delete cascade,
    tournament_id bigint  not null,
    game_number   integer not null check (game_number > 0),
    map_id        bigint  not null,
    unique (match_id, game_number),
    constraint match_game_of_match
        foreign key (match_id, tournament_id) references tournament_match (id, tournament_id)
        on delete cascade,
    constraint match_game_map_in_pool
        foreign key (tournament_id, map_id) references tournament_map (tournament_id, map_id)
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
-- heroes live in `hero_ban` and never appear as a game participant.
--
-- `side` here is NOT declared as an FK back to match_participant.side --
-- doing that would require denormalizing match_id onto this table too, one
-- level further down. The side-pairing between match_participant and
-- match_game_participant (side 0 of a game is played by side 0 of the
-- series) is an APPLICATION-level invariant, enforced by MatchResultPolicy /
-- AdminMatchService always building both lists in the same order -- the same
-- tier of guarantee "exactly 2 participants" already was before this change.
create table match_game_participant (
    game_id          bigint  not null references match_game (id) on delete cascade,
    side             integer not null check (side in (0, 1)),
    hero_id          bigint  not null references heroes (id),
    health_remaining integer not null,
    is_winner        boolean not null default false,
    primary key (game_id, side),
    unique (game_id, hero_id),
    constraint match_game_participant_loser_defeated check (is_winner or health_remaining <= 0)
);

create unique index uq_match_game_participant_winner
    on match_game_participant (game_id)
    where is_winner;

-- A hero banned out of the series. Modelled as its own table rather than a
-- `was_banned` flag on match_game_participant: a banned hero has no health
-- and no result, so storing it as a participant would force
-- health_remaining = 0, which is indistinguishable from "played and was
-- defeated" and would poison every HEALTH_REMAINING sum and SHUTOUT check.
--
-- Bans happen once, before the series starts -- not per game -- which is why
-- this references tournament_match, not match_game. `ban_type` follows this
-- file's existing check-constrained text + Kotlin enum convention (see
-- manager.rank_tier / tournament.status).
--
-- Who struck the ban is not recorded beyond its category: the BAN metric
-- prices the banned hero, never the person who banned it. Natural composite
-- key, no surrogate.
--
-- A parallel `map_ban (match_id, map_id, ban_type)` table is a plausible
-- future extension for board draft/bans -- not built now.
create table hero_ban (
    match_id bigint not null references tournament_match (id) on delete cascade,
    hero_id  bigint not null references heroes (id),
    ban_type text   not null check (ban_type in ('PRE_BAN', 'OPPONENT_BAN', 'SELF_BAN')),
    primary key (match_id, hero_id)
);

create index idx_scoring_coefficient_rule_set on scoring_coefficient (rule_set_id, sort_order);
create index idx_tournament_match_tournament on tournament_match (tournament_id, id desc);
create index idx_tournament_match_played_at on tournament_match (tournament_id, played_at desc);
create index idx_match_game_participant_hero on match_game_participant (hero_id);
create index idx_hero_ban_hero on hero_ban (hero_id);

-- ===========================================================================
-- Seed fixtures.
-- ===========================================================================

-- The full canonical hero roster from the physical game (all released
-- Unmatched sets as of this seed). Only a subset is ever priced into a given
-- tournament's pool (`tournament_hero`), but every hero the game has ever
-- printed is reference data here so an admin can pool and price any of them
-- without a migration.
insert into heroes (name) values
    ('Alice'),
    ('King Arthur'),
    ('Medusa'),
    ('Sinbad'),
    ('Bruce Lee'),
    ('Robin Hood'),
    ('Bigfoot'),
    ('InGen'),
    ('Raptors'),
    ('Sherlock Holmes'),
    ('Invisible Man'),
    ('Dracula'),
    ('Jekyll & Hyde'),
    ('Buffy'),
    ('Angel'),
    ('Spike'),
    ('Willow'),
    ('Little Red Riding Hood'),
    ('Beowulf'),
    ('Deadpool'),
    ('Achilles'),
    ('Yennenga'),
    ('Bloody Mary'),
    ('Sun Wukong'),
    ('Moon Knight'),
    ('Luke Cage'),
    ('Ghost Rider'),
    ('Daredevil'),
    ('Elektra'),
    ('Bullseye'),
    ('Dr. Ellie Sattler'),
    ('T. Rex'),
    ('Houdini'),
    ('The Genie'),
    ('Ms. Marvel'),
    ('Cloak & Dagger'),
    ('Squirrel Girl'),
    ('Black Widow'),
    ('Black Panther'),
    ('Winter Soldier'),
    ('Doctor Strange'),
    ('Spiderman'),
    ('She-Hulk'),
    ('Annie Christmas'),
    ('Golden Bat'),
    ('Nikola Tesla'),
    ('Dr. Jill Trent'),
    ('Tomoe Gozen'),
    ('Oda Nobunaga'),
    ('Shakespeare'),
    ('Titania'),
    ('Hamlet'),
    ('The Wayward Sisters'),
    ('Geralt of Rivia'),
    ('Ancient Leshen'),
    ('Ciri'),
    ('Yennefer & Triss'),
    ('Eredin'),
    ('Philippa'),
    ('Loki'),
    ('Pandora'),
    ('Chupacabra'),
    ('Blackbeard'),
    ('Leonardo'),
    ('Donatello'),
    ('Michelangelo'),
    ('Raphael'),
    ('Shredder'),
    ('Krang'),
    ('Muhammad Ali'),
    ('John Henry'),
    ('Rosie the Riveter'),
    ('George Washington'),
    ('Wyatt Earp');

-- The full canonical board pool. As with `heroes` above, only a subset is ever
-- legal for a given tournament (`tournament_map`); the rest sit here as
-- reference data an admin can pool without a migration.
insert into game_map (name) values
    ('Sarpedon'),
    ('Marmoreal'),
    ('Sherwood Forest'),
    ('Yukon'),
    ('Raptor Paddock'),
    ('Soho'),
    ('Baskerville Manor'),
    ('Sunnydale High'),
    ('The Bronze'),
    ('Heorot'),
    ('Hanging Gardens'),
    ('The Raft'),
    ('Hell''s Kitchen'),
    ('T. Rex Paddock'),
    ('King Solomon''s Mine'),
    ('Navy Pier'),
    ('Helicarrier'),
    ('Sanctum Sanctorum'),
    ('McMinnville'),
    ('Point Pleasant'),
    ('Azuchi Castle'),
    ('Globe Theatre'),
    ('Streets of Novigrad'),
    ('Naglfar'),
    ('Kaer Mohren'),
    ('Fayrlund Forest'),
    ('Venice'),
    ('Santa''s Workshop'),
    ('Tsing Shan Monastery'),
    ('Thrilla in Manilla'),
    ('New York City'),
    ('Technodrome'),
    ('The White House'),
    ('The Alamo'),
    ('Yukon-1900');

-- ---------------------------------------------------------------------------
-- Managers. NeonStrategist is the identity the dev auth stub resolves to, and
-- is flagged admin -- that is what makes the admin API reachable in local
-- dev/test with zero extra setup. No credit balances: budget is granted per
-- registration.
-- ---------------------------------------------------------------------------

insert into manager (handle, display_name, rank_tier, rank_division, is_admin) values
    ('NeonStrategist',  'Neon Strategist',  'ELITE',  'III', true),
    ('SherlockMain',    'Sherlock Main',    'GOLD',   'I',   false),
    ('MythicMind',      'Mythic Mind',      'ELITE',  'II',  false),
    ('ArthurianLegend', 'Arthurian Legend', 'LEGEND', 'I',   false);

-- ---------------------------------------------------------------------------
-- Tournaments, one per lifecycle state the Lobby renders.
--
-- Winter of Champions is deliberately left with zero entries: it is the
-- tournament the Roster Builder walkthrough registers for.
-- ---------------------------------------------------------------------------

insert into tournament (name, format, status, start_date, end_date,
                        capacity, roster_size, credit_grant) values
    ('Summer of Legends',   'BANQUEST', 'COMPLETED',         date '2026-06-05', date '2026-06-07', 128, 3, 10000),
    ('Winter of Champions', 'ARSENAL',  'REGISTRATION_OPEN', date '2026-08-14', null,               64, 3, 10000),
    ('Spring of Myths',     'BANQUEST', 'SCHEDULED',         date '2026-09-18', null,               32, 3, 10000);

-- ---------------------------------------------------------------------------
-- Hero pools and prices.
--
-- Tuned so that one premium pick plus two budget picks just fits the 10,000
-- grant while three premium picks does not -- e.g. in Summer of Legends
-- Sun Wukong 5600 + Beowulf 2400 + Sinbad 1900 = 9,900, but Sun Wukong 5600 +
-- Medusa 5100 + King Arthur 4500 = 15,200.
--
-- Summer and Winter both carry all twelve heroes, but at deliberately
-- different prices for King Arthur, Medusa and Sun Wukong: cost is per
-- tournament, and the seed should prove that rather than assert it. Spring of
-- Myths carries a narrower eight-hero pool, so "not in this tournament's pool"
-- is a reachable UNKNOWN_HERO case.
-- ---------------------------------------------------------------------------

insert into tournament_hero (tournament_id, hero_id, cost)
select t.id, h.id, v.cost
from (values
    -- Summer of Legends -- all twelve
    ('Summer of Legends',   'Alice',           4100),
    ('Summer of Legends',   'King Arthur',     4500),
    ('Summer of Legends',   'Robin Hood',      3200),
    ('Summer of Legends',   'Medusa',          5100),
    ('Summer of Legends',   'Sherlock Holmes', 3400),
    ('Summer of Legends',   'Dracula',         3800),
    ('Summer of Legends',   'Bigfoot',         2100),
    ('Summer of Legends',   'Sun Wukong',      5600),
    ('Summer of Legends',   'Achilles',        4300),
    ('Summer of Legends',   'Yennenga',        2900),
    ('Summer of Legends',   'Beowulf',         2400),
    ('Summer of Legends',   'Sinbad',          1900),
    -- Winter of Champions -- all twelve, three re-priced
    ('Winter of Champions', 'Alice',           4100),
    ('Winter of Champions', 'King Arthur',     4700),
    ('Winter of Champions', 'Robin Hood',      3200),
    ('Winter of Champions', 'Medusa',          5600),
    ('Winter of Champions', 'Sherlock Holmes', 3400),
    ('Winter of Champions', 'Dracula',         3800),
    ('Winter of Champions', 'Bigfoot',         2100),
    ('Winter of Champions', 'Sun Wukong',      5300),
    ('Winter of Champions', 'Achilles',        4300),
    ('Winter of Champions', 'Yennenga',        2900),
    ('Winter of Champions', 'Beowulf',         2400),
    ('Winter of Champions', 'Sinbad',          1900),
    -- Spring of Myths -- narrower pool
    ('Spring of Myths',     'Alice',           4000),
    ('Spring of Myths',     'King Arthur',     4600),
    ('Spring of Myths',     'Medusa',          5200),
    ('Spring of Myths',     'Sherlock Holmes', 3300),
    ('Spring of Myths',     'Sun Wukong',      5500),
    ('Spring of Myths',     'Achilles',        4400),
    ('Spring of Myths',     'Beowulf',         2500),
    ('Spring of Myths',     'Sinbad',          2000)
) as v(tournament_name, hero_name, cost)
    join tournament t on t.name = v.tournament_name
    join heroes h on h.name = v.hero_name;

insert into tournament_map (tournament_id, map_id)
select t.id, m.id
from (values
    ('Summer of Legends',   'Baskerville Manor'),
    ('Summer of Legends',   'Sherwood Forest'),
    ('Summer of Legends',   'Raptor Paddock'),
    ('Winter of Champions', 'Baskerville Manor'),
    ('Winter of Champions', 'Sherwood Forest'),
    ('Spring of Myths',     'Sherwood Forest'),
    ('Spring of Myths',     'Raptor Paddock')
) as v(tournament_name, map_name)
    join tournament t on t.name = v.tournament_name
    join game_map m on m.name = v.map_name;

-- ---------------------------------------------------------------------------
-- Scoring. One active rule set per tournament, all carrying the same weights.
--
-- CROWD_FAVOURITE is intentional: no Kotlin extractor implements it, so it is
-- the seeded proof that an unknown metric contributes zero, is dropped from
-- the leaderboard's columns, and throws nothing. Do not implement it.
--
-- sort_order fixes the leaderboard's left-to-right column order.
-- ---------------------------------------------------------------------------

insert into scoring_rule_set (tournament_id, name, is_active)
select t.id, 'Season 2026 Standard', true
from tournament t;

insert into scoring_coefficient (rule_set_id, metric, coefficient, sort_order)
select rs.id, v.metric, v.coefficient, v.sort_order
from scoring_rule_set rs
    cross join (values
        ('WIN',                10.0000, 0),
        ('HEALTH_REMAINING',    0.7500, 1),
        ('HEALTH_DIFFERENTIAL', 0.5000, 2),
        ('SHUTOUT',             3.0000, 3),
        ('BAN',                 2.0000, 4),
        ('APPEARANCE',          1.0000, 5),
        ('CROWD_FAVOURITE',     5.0000, 6)
    ) as v(metric, coefficient, sort_order);

-- ---------------------------------------------------------------------------
-- Summer of Legends: the recorded results. Thirteen matches over three
-- rounds, eight players who each play three matches -- plus a Bo3 decider
-- (match 13) shared by Rina Okafor and Dmitri Kovac, their fourth match
-- apiece; every one of the twelve pooled heroes is played at least twice.
--
-- Match ids are given explicitly, which is the one place this file does not
-- key off a natural column: `tournament_match` has none, and the game/
-- participant/ban rows below have to point at a specific match. Writing the
-- ids out also makes the invariant checkable by eye -- ids ascend with
-- played_at, so the ticker can filter on `id` while sorting on `played_at`.
-- `played_at` is itself NOT unique: parallel tables (ids 1/2, 3/4, 5/6, 7/8,
-- 9/10) share a start time, which is exactly why the polling key is the id.
--
-- Match 13 is the one multi-game series in the seed, proving Bo3 works end to
-- end without disturbing any of the twelve hand-verified single-game matches
-- above. Medusa and Achilles are deliberately reused here because the
-- `entry_slot` comment below already calls them out as "on nobody's roster",
-- so every point this match generates -- game results AND its bans -- lands
-- on zero fantasy totals, leaving every existing standings assertion exact.
-- Its three bans (Bruce Lee, Deadpool, Invisible Man) are outside Summer of
-- Legends' own `tournament_hero` pool for the same reason -- nothing in the
-- schema requires a banned or played hero to be pool-priced, only that maps
-- come from `tournament_map` (see `MatchResultPolicy.UNKNOWN_HERO`, which
-- validates against `heroes`, never `tournament_hero`). `external_link` is
-- exercised only here -- every other match leaves it null.
-- ---------------------------------------------------------------------------

insert into tournament_match (id, tournament_id, round, played_at, external_link)
select v.id, t.id, v.round, v.played_at, v.external_link
from (values
    ( 1, 1, timestamptz '2026-06-05 12:00:00+00', null),
    ( 2, 1, timestamptz '2026-06-05 12:00:00+00', null),
    ( 3, 1, timestamptz '2026-06-05 14:30:00+00', null),
    ( 4, 1, timestamptz '2026-06-05 14:30:00+00', null),
    ( 5, 2, timestamptz '2026-06-06 11:00:00+00', null),
    ( 6, 2, timestamptz '2026-06-06 11:00:00+00', null),
    ( 7, 2, timestamptz '2026-06-06 13:30:00+00', null),
    ( 8, 2, timestamptz '2026-06-06 13:30:00+00', null),
    ( 9, 3, timestamptz '2026-06-07 10:00:00+00', null),
    (10, 3, timestamptz '2026-06-07 10:00:00+00', null),
    (11, 3, timestamptz '2026-06-07 12:30:00+00', null),
    (12, 3, timestamptz '2026-06-07 15:00:00+00', null),
    (13, 3, timestamptz '2026-06-07 16:30:00+00', 'https://challonge.com/example-bo3-decider')
) as v(id, round, played_at, external_link)
    join tournament t on t.name = 'Summer of Legends';

select setval('tournament_match_id_seq', (select max(id) from tournament_match));

-- Two sides per match, for the whole series. Eight named competitors, each
-- playing three matches (Rina Okafor and Dmitri Kovac also play the match 13
-- decider) -- names carried as labels, not rows in a table (see the comment
-- on `match_participant.player_label`).
insert into match_participant (match_id, side, player_label)
select v.match_id, v.side, v.player_label
from (values
    ( 1, 0, 'Tomas Ferreira'),    ( 1, 1, 'Hana Sato'),
    ( 2, 0, 'Rina Okafor'),       ( 2, 1, 'Jonas Lindqvist'),
    ( 3, 0, 'Aurelie Blanc'),     ( 3, 1, 'Dmitri Kovac'),
    ( 4, 0, 'Miles Ashworth'),    ( 4, 1, 'Priya Raghunathan'),
    ( 5, 0, 'Rina Okafor'),       ( 5, 1, 'Tomas Ferreira'),
    ( 6, 0, 'Aurelie Blanc'),     ( 6, 1, 'Miles Ashworth'),
    ( 7, 0, 'Hana Sato'),         ( 7, 1, 'Priya Raghunathan'),
    ( 8, 0, 'Dmitri Kovac'),      ( 8, 1, 'Jonas Lindqvist'),
    ( 9, 0, 'Rina Okafor'),       ( 9, 1, 'Dmitri Kovac'),
    (10, 0, 'Hana Sato'),         (10, 1, 'Aurelie Blanc'),
    (11, 0, 'Miles Ashworth'),    (11, 1, 'Tomas Ferreira'),
    (12, 0, 'Jonas Lindqvist'),   (12, 1, 'Priya Raghunathan'),
    (13, 0, 'Rina Okafor'),       (13, 1, 'Dmitri Kovac')
) as v(match_id, side, player_label);

-- One game per match for the twelve original single-game matches (same board
-- each carried before this split), plus three games for match 13's Bo3.
insert into match_game (match_id, tournament_id, game_number, map_id)
select v.match_id, t.id, 1, m.id
from (values
    ( 1, 'Baskerville Manor'),
    ( 2, 'Sherwood Forest'),
    ( 3, 'Raptor Paddock'),
    ( 4, 'Baskerville Manor'),
    ( 5, 'Sherwood Forest'),
    ( 6, 'Raptor Paddock'),
    ( 7, 'Baskerville Manor'),
    ( 8, 'Sherwood Forest'),
    ( 9, 'Raptor Paddock'),
    (10, 'Baskerville Manor'),
    (11, 'Sherwood Forest'),
    (12, 'Raptor Paddock')
) as v(match_id, map_name)
    join tournament t on t.name = 'Summer of Legends'
    join game_map m on m.name = v.map_name;

insert into match_game (match_id, tournament_id, game_number, map_id)
select 13, t.id, v.game_number, m.id
from (values
    (1, 'Sherwood Forest'),
    (2, 'Raptor Paddock'),
    (3, 'Baskerville Manor')
) as v(game_number, map_name)
    join tournament t on t.name = 'Summer of Legends'
    join game_map m on m.name = v.map_name;

-- Match 6 is the SHUTOUT: Bigfoot finishes on 11 health, Beowulf on 0.
-- Every losing hero finishes on 0 or less health. Every game has a winner --
-- there is no drawn result to seed, see match_game_participant's comment.
insert into match_game_participant (game_id, side, hero_id, health_remaining, is_winner)
select mg.id, v.side, h.id, v.health_remaining, v.is_winner
from (values
    ( 1, 0, 'Sun Wukong',       9, true),
    ( 1, 1, 'Alice',            0, false),
    ( 2, 0, 'Robin Hood',       6, true),
    ( 2, 1, 'Achilles',         0, false),
    ( 3, 0, 'Yennenga',         5, true),
    ( 3, 1, 'King Arthur',      0, false),
    ( 4, 0, 'Sherlock Holmes',  7, true),
    ( 4, 1, 'Dracula',          0, false),
    ( 5, 0, 'Medusa',           8, true),
    ( 5, 1, 'Sun Wukong',       0, false),
    -- shutout
    ( 6, 0, 'Bigfoot',         11, true),
    ( 6, 1, 'Beowulf',          0, false),
    ( 7, 0, 'Alice',            6, true),
    ( 7, 1, 'Sinbad',           0, false),
    ( 8, 0, 'King Arthur',     10, true),
    ( 8, 1, 'Robin Hood',       0, false),
    ( 9, 0, 'Medusa',           7, true),
    ( 9, 1, 'Achilles',         0, false),
    (10, 0, 'Beowulf',          8, true),
    (10, 1, 'Bigfoot',          0, false),
    (11, 0, 'Sherlock Holmes',  7, true),
    (11, 1, 'Dracula',          0, false),
    (12, 0, 'Yennenga',         9, true),
    (12, 1, 'Sinbad',           0, false)
) as v(match_id, side, hero_name, health_remaining, is_winner)
    join match_game mg on mg.match_id = v.match_id and mg.game_number = 1
    join heroes h on h.name = v.hero_name;

-- Match 13: Medusa wins game 1, Achilles ties the series in game 2, Medusa
-- takes the decider in game 3.
insert into match_game_participant (game_id, side, hero_id, health_remaining, is_winner)
select mg.id, v.side, h.id, v.health_remaining, v.is_winner
from (values
    (1, 0, 'Medusa',   6, true),  (1, 1, 'Achilles', 0, false),
    (2, 0, 'Medusa',   0, false), (2, 1, 'Achilles', 5, true),
    (3, 0, 'Medusa',   3, true),  (3, 1, 'Achilles', 0, false)
) as v(game_number, side, hero_name, health_remaining, is_winner)
    join match_game mg on mg.match_id = 13 and mg.game_number = v.game_number
    join heroes h on h.name = v.hero_name;

-- One or two bans per match, never naming a hero that then played it. Heroes
-- on the seeded rosters (Bigfoot, Beowulf, Alice, Robin Hood, Sherlock
-- Holmes, Dracula, King Arthur, Yennenga, Sun Wukong) are all banned
-- somewhere, so the BAN metric has real work to do. `ban_type` here is
-- illustrative fixture data, not a rule the schema enforces -- nothing links
-- the category distribution to `tournament.format`.
insert into hero_ban (match_id, hero_id, ban_type)
select v.match_id, h.id, v.ban_type
from (values
    ( 1, 'Medusa',          'PRE_BAN'),
    ( 1, 'Bigfoot',         'OPPONENT_BAN'),
    ( 2, 'Sun Wukong',      'PRE_BAN'),
    ( 3, 'Beowulf',         'PRE_BAN'),
    ( 3, 'Dracula',         'OPPONENT_BAN'),
    ( 4, 'Medusa',          'PRE_BAN'),
    ( 5, 'Alice',           'PRE_BAN'),
    ( 5, 'Robin Hood',      'OPPONENT_BAN'),
    ( 6, 'Sun Wukong',      'PRE_BAN'),
    ( 7, 'Medusa',          'PRE_BAN'),
    ( 7, 'Sherlock Holmes', 'OPPONENT_BAN'),
    ( 8, 'Beowulf',         'PRE_BAN'),
    ( 9, 'Bigfoot',         'PRE_BAN'),
    ( 9, 'Sun Wukong',      'OPPONENT_BAN'),
    (10, 'Alice',           'PRE_BAN'),
    (11, 'Medusa',          'PRE_BAN'),
    (11, 'Yennenga',        'OPPONENT_BAN'),
    (12, 'King Arthur',     'PRE_BAN'),
    (12, 'Alice',           'OPPONENT_BAN'),
    (13, 'Bruce Lee',       'PRE_BAN'),
    (13, 'Deadpool',        'OPPONENT_BAN'),
    (13, 'Invisible Man',   'SELF_BAN')
) as v(match_id, hero_name, ban_type)
    join heroes h on h.name = v.hero_name;

-- ---------------------------------------------------------------------------
-- Fantasy entries. All four managers played Summer of Legends and locked a
-- full three-hero roster inside the 10,000 grant.
--
-- `credit_grant` is copied off the tournament, which is what a real
-- registration does.
-- ---------------------------------------------------------------------------

insert into tournament_entry (tournament_id, manager_id, status, credit_grant, registered_at, locked_at)
select t.id,
       m.id,
       'LOCKED',
       t.credit_grant,
       timestamptz '2026-06-01 09:00:00+00',
       timestamptz '2026-06-04 18:00:00+00'
from tournament t
    cross join manager m
where t.name = 'Summer of Legends';

-- Bigfoot is shared by NeonStrategist and MythicMind, and Beowulf by
-- SherlockMain and ArthurianLegend: a hero-match is scored once and then
-- counted into every entry that holds it, and these two pairs are what
-- exercises that. Medusa and Achilles are on nobody's roster.
insert into entry_slot (entry_id, slot_index, hero_id)
select e.id, v.slot_index, h.id
from (values
    -- 4100 + 3200 + 2100 = 9,400
    ('NeonStrategist',  0, 'Alice'),
    ('NeonStrategist',  1, 'Robin Hood'),
    ('NeonStrategist',  2, 'Bigfoot'),
    -- 3400 + 3800 + 2400 = 9,600
    ('SherlockMain',    0, 'Sherlock Holmes'),
    ('SherlockMain',    1, 'Dracula'),
    ('SherlockMain',    2, 'Beowulf'),
    -- 5600 + 1900 + 2100 = 9,600
    ('MythicMind',      0, 'Sun Wukong'),
    ('MythicMind',      1, 'Sinbad'),
    ('MythicMind',      2, 'Bigfoot'),
    -- 4500 + 2900 + 2400 = 9,800
    ('ArthurianLegend', 0, 'King Arthur'),
    ('ArthurianLegend', 1, 'Yennenga'),
    ('ArthurianLegend', 2, 'Beowulf')
) as v(handle, slot_index, hero_name)
    join manager mg on mg.handle = v.handle
    join tournament t on t.name = 'Summer of Legends'
    join tournament_entry e on e.manager_id = mg.id and e.tournament_id = t.id
    join heroes h on h.name = v.hero_name;
