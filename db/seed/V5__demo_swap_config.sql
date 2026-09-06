-- ===========================================================================
-- Demo swap configuration.
--
-- Dev/test only, like the rest of `db/seed`. This is a separate migration
-- rather than an edit to V3 because changing an applied migration's checksum
-- stops Flyway -- and so the app -- against any database that already ran it.
-- A later squash folds it back into V3, which is where it belongs.
--
-- Deliberately no `roster_swaps` rows. Seeding a swap would move Summer of
-- Legends' standings totals, which the integration suite asserts exactly, and
-- would make the fixture a worse illustration of the plain case. Round-scoped
-- scoring is covered by tests that insert their own swaps.
-- ===========================================================================

-- Summer of Legends is COMPLETED with results recorded in rounds 1 through 3,
-- so the round it is "in" is its last one. It gets no allowance: the tournament
-- is over, and a finished fixture should stay finished.
update tournaments
set current_round = 3
where name = 'Summer of Legends';

-- Winter of Champions is the walkthrough tournament -- REGISTRATION_OPEN with
-- no entries yet, which makes it the one an admin can drive end to end. One
-- swap per round is enough to exercise the allowance, the carry-over and the
-- limit without needing a large roster.
update tournaments
set swaps_per_round = 1
where name = 'Winter of Champions';
