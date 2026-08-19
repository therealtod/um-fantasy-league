-- Which side a ban came out of.
--
-- `hero_ban` recorded *what* was struck and under which category, but not
-- whose draft it was struck from -- V1's own comment on the table says as
-- much ("Who struck the ban is not recorded beyond its category"). That
-- comment is stale from here on; it is left in place because V1 is the
-- squashed baseline and editing it fails Flyway's checksum validation on any
-- database that has already run it, which stops the application from booting
-- rather than merely warning. It gets corrected at the next squash.
--
-- The information was never missing upstream, only discarded: the scraper
-- reads its "Self ban" / "Opp. ban" chips inside a side's own group
-- (`ScrapedSide.bans`), and `MatchImportService` flattened both sides into one
-- list because that is all this table could hold.
--
-- `side` is the side that *owned* the hero -- the draft it was struck out of.
-- `ban_type` keeps saying who cast it: SELF_BAN is that side striking one of
-- its own, OPPONENT_BAN is the other side striking it. A PRE_BAN happens
-- before sides are assigned and so belongs to neither, which the second check
-- enforces.
--
-- Nullable, and not only to express a pre-ban: every row written before this
-- migration has no side and cannot be given one after the fact. A typed ban
-- without a side therefore stays legal rather than making an already-recorded
-- match uncorrectable -- the same reason MatchResultPolicy.BAN_SIDE_INVALID
-- rejects only an impossible side, never a missing one. Tightening this to
-- "every typed ban names a side" would have to be a migration that can first
-- attribute the rows already in the table.
--
-- The primary key stays (match_id, hero_id): a hero is banned at most once per
-- series however many sides wanted it, which is exactly what
-- MatchRule.DUPLICATE_BAN already means.
alter table hero_ban
    add column side integer,
    add constraint hero_ban_side_range check (side in (0, 1)),
    add constraint hero_ban_pre_ban_has_no_side check (side is null or ban_type <> 'PRE_BAN');

comment on column hero_ban.side is
    'The side whose draft this hero was struck out of (0 or 1), or null for a PRE_BAN -- '
    'struck before sides were assigned -- and for rows recorded before the column existed. '
    'ban_type says who cast the ban; this says whose hero it was.';
