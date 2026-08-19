-- ===========================================================================
-- Whose draft each demo ban came out of.
--
-- A separate seed file rather than an edit to `V3__demo_fixtures.sql`, for the
-- same mechanical reason `V6__demo_draft_picks.sql` is one: Flyway orders
-- every location by version, so V3 runs long before `hero_ban.side` exists in
-- V7. Editing V3 in place would also fail checksum validation on every
-- database that has already run it, which stops the application from booting.
-- Same `db/seed` location, so this still only loads under `dev` and `test`.
-- ===========================================================================

-- The 13 PRE_BANs V3 inserts are left alone: a pre-ban is struck before sides
-- are assigned, so a null side is the right answer for them and V7's
-- `hero_ban_pre_ban_has_no_side` check enforces it.
--
-- The 9 typed bans get a side each. Which side is fixture colour rather than
-- fact -- V3 already says its `ban_type` distribution is illustrative -- so
-- these alternate to keep both sides represented. Match 13, the Bo3, is the
-- one to look at: its OPPONENT_BAN and its SELF_BAN sit on opposite sides, so
-- the fixture carries a hero struck by the enemy and a hero struck by its own
-- side in the same series.
--
-- None of these heroes appears in `match_hero_pick` for its match (a ban and a
-- pick are disjoint -- `MatchRule.BANNED_HERO_DRAFTED`, asserted in
-- SchemaAndSeedTest), so no side assignment here can contradict a draft.
update hero_ban hb
set side = v.side
from (values
    ( 1, 'Bigfoot',         0),
    ( 3, 'Dracula',         1),
    ( 5, 'Robin Hood',      0),
    ( 7, 'Sherlock Holmes', 1),
    ( 9, 'Sun Wukong',      0),
    (11, 'Yennenga',        1),
    (12, 'Alice',           0),
    (13, 'Deadpool',        1),
    (13, 'Invisible Man',   0)
) as v(match_id, hero_name, side)
    join heroes h on h.name = v.hero_name
where hb.match_id = v.match_id
  and hb.hero_id = h.id;
