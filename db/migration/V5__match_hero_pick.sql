-- ===========================================================================
-- The draft half of a match: the heroes each side actually drafted.
--
-- A match's draft was only half recorded. `hero_ban` stored the heroes struck
-- out of a series, but the heroes *taken* existed nowhere except implicitly,
-- as whatever `match_game_participant` happened to field -- so a hero drafted
-- and then never played was indistinguishable from a hero nobody drafted at
-- all. That made "featured in this match" and "played a game in this match"
-- the same fact, which is exactly what the APPEARANCE metric now needs to tell
-- apart: a hero that survives the ban phase and gets drafted has appeared,
-- whether or not it ever hit the table.
--
-- A separate table from `hero_ban` rather than one draft table with a kind
-- column, because the two carry different data: a pick has an owning `side`
-- and no category, a ban has a category (`PRE_BAN`/`OPPONENT_BAN`/`SELF_BAN`)
-- and no side -- a pre-ban belongs to neither. A hero cannot be both; that is
-- enforced in Kotlin as `MatchRule.BANNED_HERO_DRAFTED`, alongside
-- `PLAYED_HERO_NOT_DRAFTED`, which is what keeps a recorded draft complete.
--
-- Deliberately no `unique (match_id, hero_id)`. Games in a series are
-- independent -- side 0 may pilot a hero in game 1 and side 1 the same hero in
-- game 2, which `match_game_participant`'s per-game `unique (game_id,
-- hero_id)` allows -- so a cross-side unique here would retroactively outlaw a
-- result the schema accepts today. APPEARANCE de-duplicates by hero instead,
-- so a hero drafted by both sides still scores once for the match.
-- ===========================================================================

create table match_hero_pick (
    match_id bigint  not null references tournament_match (id) on delete cascade,
    side     integer not null check (side in (0, 1)),
    hero_id  bigint  not null references heroes (id),
    primary key (match_id, side, hero_id)
);

comment on table match_hero_pick is
    'A hero one side drafted for one series -- the picks half of the draft, with hero_ban as the '
    'bans half. A drafted hero need never have played: that is the whole point of recording it, '
    'and it is what APPEARANCE scores.';

create index idx_match_hero_pick_hero on match_hero_pick (hero_id);

-- Backfill: every hero already recorded as playing a game was, necessarily,
-- drafted by the side that played it. Without this an existing match fails
-- PLAYED_HERO_NOT_DRAFTED the moment an admin next corrects it.
insert into match_hero_pick (match_id, side, hero_id)
select distinct mg.match_id, mgp.side, mgp.hero_id
from match_game_participant mgp
    join match_game mg on mg.id = mgp.game_id;
