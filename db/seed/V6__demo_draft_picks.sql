-- ===========================================================================
-- The picks half of the demo matches' drafts.
--
-- A separate seed file rather than more of `V3__demo_fixtures.sql` for a
-- purely mechanical reason: Flyway orders every location by version, so V3
-- runs before `match_hero_pick` is created in V5. Same `db/seed` location,
-- so it still only loads under `dev` and `test`.
-- ===========================================================================

-- Every hero that played was, necessarily, drafted by the side that played it,
-- and a recorded draft has to be complete (`MatchRule.PLAYED_HERO_NOT_DRAFTED`)
-- -- so derive these from the games rather than restating 26 rows by hand.
-- `on conflict do nothing` because V5's own backfill already inserted exactly
-- these rows against a database that had the demo matches when it ran; this
-- file has to produce the same result either way.
insert into match_hero_pick (match_id, side, hero_id)
select distinct mg.match_id, mgp.side, mgp.hero_id
from match_game_participant mgp
    join match_game mg on mg.id = mgp.game_id
on conflict (match_id, side, hero_id) do nothing;

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
insert into match_hero_pick (match_id, side, hero_id)
select 13, v.side, h.id
from (values
    (0, 'Tomoe Gozen'),
    (1, 'Nikola Tesla')
) as v(side, hero_name)
    join heroes h on h.name = v.hero_name;
