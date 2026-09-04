-- ===========================================================================
-- Demo/dev fixtures -- NOT part of the default migration path.
--
-- This file lives outside `db/migration` on purpose. A default start (no
-- profile) or the `prod` profile migrates only the schema in
-- `V1__core_schema.sql` plus the canonical heroes and boards in
-- `V2__reference_data.sql`, and ends up with a league that has never been
-- played: zero tournaments, zero managers, zero recorded results. Only a
-- dev-shaped Flyway invocation adds this file's location too (see AGENTS.md's
-- Commands section on why that's decided by the invocation, not by the
-- backend's own profile) -- which is what pulls this file in for local dev
-- and for the test suite.
--
-- Nothing here writes `heroes` or `game_maps`. Those are reference data, not
-- fixtures, so they migrate in every profile from `V2__reference_data.sql`;
-- this file only *references* them, and would fail loudly on a name that
-- migration does not carry.
--
-- The fixture: three tournaments in three lifecycle states, one of them
-- (Summer of Legends) with a complete recorded result set. Reference rows are
-- joined by their natural key (heroes.name, game_maps.name, managers.handle,
-- tournaments.name) rather than by assumed serial ids, which is why those
-- columns are unique. The one exception is `tournament_matches`, which has no
-- natural key -- see the comment there.
--
-- NeonStrategist is flagged admin (see below) -- the only such manager here,
-- and it is also seeded first, so it lands on id 1: the default
-- `VITE_DEV_MANAGER_ID` (see README) a fresh frontend checkout already sends.
-- That is a seeding convenience, not a backend fallback -- the dev auth stub
-- (`auth::dev::resolve`) never guesses at an identity for a request with no
-- `X-Manager-Id` header; it treats it as anonymous, exactly as `prod` treats
-- one with no bearer token (see AGENTS.md's Profiles table). Seeded manager
-- ids are 1 through 4.
--
-- Integration tests assert on these numbers exactly. Changing a seeded price
-- or result means updating them.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- Managers. NeonStrategist is flagged admin -- and, being inserted first,
-- lands on id 1, the default `VITE_DEV_MANAGER_ID` a fresh frontend checkout
-- already sends -- so the admin API is reachable in local dev/test with zero
-- extra setup. No credit balances: budget is granted per registration.
-- ---------------------------------------------------------------------------

insert into managers (handle, display_name, is_admin) values
    ('NeonStrategist',  'Neon Strategist',  true),
    ('SherlockMain',    'Sherlock Main',    false),
    ('MythicMind',      'Mythic Mind',      false),
    ('ArthurianLegend', 'Arthurian Legend', false);

-- ---------------------------------------------------------------------------
-- Tournaments, one per lifecycle state the Lobby renders.
--
-- Winter of Champions is deliberately left with zero entries: it is the
-- tournament the Roster Builder walkthrough registers for.
-- ---------------------------------------------------------------------------

insert into tournaments (name, format, status, start_date, end_date,
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

insert into tournament_heroes (tournament_id, hero_id, cost)
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
    join tournaments t on t.name = v.tournament_name
    join heroes h on h.name = v.hero_name;

insert into tournament_maps (tournament_id, map_id)
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
    join tournaments t on t.name = v.tournament_name
    join game_maps m on m.name = v.map_name;

-- ---------------------------------------------------------------------------
-- Scoring. One active rule set per tournament, all carrying the same weights.
--
-- CROWD_FAVOURITE is intentional: no extractor in `umfl_domain::match_metrics`
-- implements it, so it is the seeded proof that an unknown metric contributes
-- zero, is dropped from the leaderboard's columns, and throws nothing. Do not
-- implement it.
--
-- sort_order fixes the leaderboard's left-to-right column order.
-- ---------------------------------------------------------------------------

insert into scoring_rule_sets (tournament_id, name, is_active)
select t.id, 'Season 2026 Standard', true
from tournaments t;

insert into scoring_coefficients (rule_set_id, metric, coefficient, sort_order)
select rs.id, v.metric, v.coefficient, v.sort_order
from scoring_rule_sets rs
    cross join (values
        ('WIN',                10.0000, 0),
        ('HEALTH_REMAINING',    0.7500, 1),
        ('HEALTH_DIFFERENTIAL', 0.5000, 2),
        ('SHUTOUT',             3.0000, 3),
        ('SELF_BAN',            2.0000, 4),
        ('OPPONENT_BAN',        2.0000, 5),
        ('APPEARANCE',          1.0000, 6),
        ('CROWD_FAVOURITE',     5.0000, 7)
    ) as v(metric, coefficient, sort_order);

-- ---------------------------------------------------------------------------
-- Summer of Legends: the recorded results. Thirteen matches over three
-- rounds, eight players who each play three matches -- plus a Bo3 decider
-- (match 13) shared by Rina Okafor and Dmitri Kovac, their fourth match
-- apiece; every one of the twelve pooled heroes is played at least twice.
--
-- Match ids are given explicitly, which is the one place this file does not
-- key off a natural column: `tournament_matches` has none, and the game/
-- participant/ban rows below have to point at a specific match. Writing the
-- ids out also makes the invariant checkable by eye -- ids ascend with
-- played_at, so the ticker can filter on `id` while sorting on `played_at`.
-- `played_at` is itself NOT unique: parallel tables (ids 1/2, 3/4, 5/6, 7/8,
-- 9/10) share a start time, which is exactly why the polling key is the id.
--
-- Match 13 is the one multi-game series in the seed, proving Bo3 works end to
-- end without disturbing any of the twelve hand-verified single-game matches
-- above. Medusa and Achilles are deliberately reused here because the
-- `entry_slots` comment below already calls them out as "on nobody's roster",
-- so every point this match generates -- game results AND its bans -- lands
-- on zero fantasy totals, leaving every existing standings assertion exact.
-- Its three bans (Bruce Lee, Deadpool, Invisible Man) are outside Summer of
-- Legends' own `tournament_heroes` pool for the same reason -- nothing in the
-- schema requires a banned or played hero to be pool-priced, only that maps
-- come from `tournament_maps` (see `MatchRule::UnknownHero`, which
-- validates against `heroes`, never `tournament_heroes`). `external_link` is
-- required (`V1__core_schema.sql`); match 13 carries a real one, and every
-- other match gets the same synthetic `urn:umfl:match:<id>` placeholder a
-- hand-typed match with no page anywhere gets.
-- ---------------------------------------------------------------------------

insert into tournament_matches (id, tournament_id, round, played_at, external_link)
select v.id, t.id, v.round, v.played_at,
       coalesce(v.external_link, 'urn:umfl:match:' || v.id)
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
    join tournaments t on t.name = 'Summer of Legends';

select setval('tournament_matches_id_seq', (select max(id) from tournament_matches));

-- Two sides per match, for the whole series. Eight named competitors, each
-- playing three matches (Rina Okafor and Dmitri Kovac also play the match 13
-- decider) -- names carried as labels, not rows in a table (see the comment
-- on `match_participants.player_label`).
insert into match_participants (match_id, side, player_label)
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
insert into match_games (match_id, tournament_id, game_number, map_id)
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
    join tournaments t on t.name = 'Summer of Legends'
    join game_maps m on m.name = v.map_name;

insert into match_games (match_id, tournament_id, game_number, map_id)
select 13, t.id, v.game_number, m.id
from (values
    (1, 'Sherwood Forest'),
    (2, 'Raptor Paddock'),
    (3, 'Baskerville Manor')
) as v(game_number, map_name)
    join tournaments t on t.name = 'Summer of Legends'
    join game_maps m on m.name = v.map_name;

-- Match 6 is the SHUTOUT: Bigfoot finishes on 11 health, Beowulf on 0.
-- Every losing hero finishes on 0 or less health. Every game has a winner --
-- there is no drawn result to seed, see match_game_participants's comment.
insert into match_game_participants (game_id, side, hero_id, health_remaining, is_winner)
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
    join match_games mg on mg.match_id = v.match_id and mg.game_number = 1
    join heroes h on h.name = v.hero_name;

-- Match 13: Medusa wins game 1, Achilles ties the series in game 2, Medusa
-- takes the decider in game 3.
insert into match_game_participants (game_id, side, hero_id, health_remaining, is_winner)
select mg.id, v.side, h.id, v.health_remaining, v.is_winner
from (values
    (1, 0, 'Medusa',   6, true),  (1, 1, 'Achilles', 0, false),
    (2, 0, 'Medusa',   0, false), (2, 1, 'Achilles', 5, true),
    (3, 0, 'Medusa',   3, true),  (3, 1, 'Achilles', 0, false)
) as v(game_number, side, hero_name, health_remaining, is_winner)
    join match_games mg on mg.match_id = 13 and mg.game_number = v.game_number
    join heroes h on h.name = v.hero_name;

-- ---------------------------------------------------------------------------
-- The picks half of every match's draft. Every hero that played was,
-- necessarily, drafted by the side that played it, so this is derived from
-- the games above rather than restating 26 rows by hand.
-- ---------------------------------------------------------------------------

insert into match_hero_picks (match_id, side, hero_id)
select distinct mg.match_id, mgp.side, mgp.hero_id
from match_game_participants mgp
    join match_games mg on mg.id = mgp.game_id;

-- Two heroes drafted and never fielded, so the fixture exercises the whole
-- point of storing picks: APPEARANCE credits a hero for surviving the draft,
-- not for reaching the table.
--
-- Deliberately on match 13 -- the only Bo3, whose Medusa and Achilles are on
-- nobody's roster -- and deliberately naming heroes nobody drafted into a
-- roster either, so these two rows demonstrate the metric without moving a
-- single seeded leaderboard total. Neither is among match 13's bans (Bruce
-- Lee, Deadpool, Invisible Man), which would make the pick a
-- `BANNED_HERO_DRAFTED` violation, and like those bans neither needs to be in
-- the tournament's priced pool.
insert into match_hero_picks (match_id, side, hero_id)
select 13, v.side, h.id
from (values
    (0, 'Tomoe Gozen'),
    (1, 'Nikola Tesla')
) as v(side, hero_name)
    join heroes h on h.name = v.hero_name;

-- One or two bans per match, never naming a hero that then played it. Heroes
-- on the seeded rosters (Bigfoot, Beowulf, Alice, Robin Hood, Sherlock
-- Holmes, Dracula, King Arthur, Yennenga, Sun Wukong) are all banned
-- somewhere, so the SELF_BAN/OPPONENT_BAN metrics have real work to do.
-- `ban_type` here is illustrative fixture data, not a rule the schema
-- enforces -- nothing links the category distribution to `tournaments.format`.
--
-- `side` is the side whose draft the hero was struck out of (see
-- `hero_bans.side` in V1). The 13 PRE_BANs carry no side, since a pre-ban
-- precedes side assignment; the other 9 typed bans alternate sides to keep
-- both represented -- match 13's decider is the one to look at, its
-- OPPONENT_BAN and SELF_BAN sitting on opposite sides so the fixture carries
-- a hero struck by the enemy and a hero struck by its own side in the same
-- series. Which side is fixture colour rather than fact.
insert into hero_bans (match_id, hero_id, ban_type, side)
select v.match_id, h.id, v.ban_type, v.side
from (values
    ( 1, 'Medusa',          'PRE_BAN',      null),
    ( 1, 'Bigfoot',         'OPPONENT_BAN', 0),
    ( 2, 'Sun Wukong',      'PRE_BAN',      null),
    ( 3, 'Beowulf',         'PRE_BAN',      null),
    ( 3, 'Dracula',         'OPPONENT_BAN', 1),
    ( 4, 'Medusa',          'PRE_BAN',      null),
    ( 5, 'Alice',           'PRE_BAN',      null),
    ( 5, 'Robin Hood',      'OPPONENT_BAN', 0),
    ( 6, 'Sun Wukong',      'PRE_BAN',      null),
    ( 7, 'Medusa',          'PRE_BAN',      null),
    ( 7, 'Sherlock Holmes', 'OPPONENT_BAN', 1),
    ( 8, 'Beowulf',         'PRE_BAN',      null),
    ( 9, 'Bigfoot',         'PRE_BAN',      null),
    ( 9, 'Sun Wukong',      'OPPONENT_BAN', 0),
    (10, 'Alice',           'PRE_BAN',      null),
    (11, 'Medusa',          'PRE_BAN',      null),
    (11, 'Yennenga',        'OPPONENT_BAN', 1),
    (12, 'King Arthur',     'PRE_BAN',      null),
    (12, 'Alice',           'OPPONENT_BAN', 0),
    (13, 'Bruce Lee',       'PRE_BAN',      null),
    (13, 'Deadpool',        'OPPONENT_BAN', 1),
    (13, 'Invisible Man',   'SELF_BAN',     0)
) as v(match_id, hero_name, ban_type, side)
    join heroes h on h.name = v.hero_name;

-- ---------------------------------------------------------------------------
-- Fantasy entries. All four managers played Summer of Legends and locked a
-- full three-hero roster inside the 10,000 grant.
--
-- `credit_grant` is copied off the tournament, which is what a real
-- registration does.
-- ---------------------------------------------------------------------------

insert into tournament_entries (tournament_id, manager_id, status, credit_grant, registered_at, locked_at)
select t.id,
       m.id,
       'LOCKED',
       t.credit_grant,
       timestamptz '2026-06-01 09:00:00+00',
       timestamptz '2026-06-04 18:00:00+00'
from tournaments t
    cross join managers m
where t.name = 'Summer of Legends';

-- Bigfoot is shared by NeonStrategist and MythicMind, and Beowulf by
-- SherlockMain and ArthurianLegend: a hero-match is scored once and then
-- counted into every entry that holds it, and these two pairs are what
-- exercises that. Medusa and Achilles are on nobody's roster.
insert into entry_slots (entry_id, slot_index, hero_id)
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
    join managers mg on mg.handle = v.handle
    join tournaments t on t.name = 'Summer of Legends'
    join tournament_entries e on e.manager_id = mg.id and e.tournament_id = t.id
    join heroes h on h.name = v.hero_name;
