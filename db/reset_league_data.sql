-- ===========================================================================
-- Manual utility: wipe every table of league/result data, leaving the
-- canonical hero and board catalogue (`heroes`, `game_maps`) untouched.
--
-- NOT a Flyway migration -- it deliberately does not live in `db/migration`
-- or `db/seed`, and does not follow the `V<n>__<name>.sql` naming convention,
-- so Flyway never scans or applies it. Run it by hand against a database you
-- want reset without dropping and re-migrating from scratch, e.g.:
--
--   docker compose exec -T db psql -U umfl -d umfl -f - < db/reset_league_data.sql
--
-- For a full reset that also re-seeds the demo fixtures (dev/test) from a
-- clean Flyway run, drop the volume instead:
--
--   docker compose down -v && docker compose up -d db
-- ===========================================================================

truncate table
    managers,
    tournaments,
    tournament_heroes,
    tournament_maps,
    tournament_entries,
    entry_slots,
    scoring_rule_sets,
    scoring_coefficients,
    tournament_matches,
    match_participants,
    match_games,
    match_game_participants,
    hero_bans,
    match_hero_picks
restart identity cascade;
