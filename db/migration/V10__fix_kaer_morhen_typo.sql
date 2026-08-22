-- V2 seeded this board as 'Kaer Mohren' -- a transposition typo. The Witcher
-- stronghold, and the map's name everywhere it's displayed, is 'Kaer Morhen'.
--
-- A forward migration rather than an edit to V2: that file is already applied
-- everywhere and editing it fails Flyway's checksum validation (see
-- AGENTS.md). Runs in every profile, like the rest of the reference-data
-- catalogue -- a deployed database needs the correct name too, not just dev/test.
update game_map
set name = 'Kaer Morhen'
where name = 'Kaer Mohren';
