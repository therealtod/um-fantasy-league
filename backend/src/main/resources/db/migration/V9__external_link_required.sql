-- A match's source link is now required, and unique within its tournament.
--
-- The admin match import turns a tabletopleague.com URL into a reviewable
-- draft, and `external_link` stores the URL it came from. That was the whole
-- of the duplicate check: `MatchResultQuery.findIdByExternalLink` looked for a
-- match already recorded against the URL and `MatchImportPanel` rendered the
-- answer as a warning that did not stop the admin recording a second copy.
-- Importing the same match twice therefore double-counted every APPEARANCE,
-- WIN and ban it carried, and nothing surfaced it until someone doubted the
-- standings.
--
-- A warning cannot be the guard here, because the damage is silent. The
-- database becomes the authority instead.
--
-- Scoped to (tournament_id, external_link) rather than made globally unique,
-- for two reasons: the tournament is this schema's unit of scoping everywhere
-- else, and the existing preview check was already per tournament -- a global
-- constraint would let the wizard report "no duplicate" and then fail the save.
--
-- The correction path is undisturbed. `AdminMatchService.correct` re-saves the
-- same aggregate root, so `tournament_match` takes an UPDATE in place and a
-- correction that reuses its own URL never meets the index. Only *moving* one
-- match's link onto another's now conflicts, which is the mistake worth
-- catching.
--
-- V1's comment on the column ("Optional link ... no validation beyond
-- nullability") and V3's note that "external_link is exercised only here --
-- every other match leaves it null" are both stale from here on. They are left
-- in place because editing an applied migration fails Flyway's checksum
-- validation and stops the application booting; they get corrected at the next
-- squash.

-- Backfill before the constraints, not after: rows predating this migration
-- have no link and cannot be given a real one after the fact. A synthetic urn
-- keyed on the primary key is unique by construction, so it satisfies the
-- index as well as the not-null. This runs in every profile -- a deployed
-- database may hold recorded matches that were entered by hand.
update tournament_match
set external_link = 'urn:umfl:match:' || id
where external_link is null;

create unique index uq_tournament_match_external_link
    on tournament_match (tournament_id, external_link);

alter table tournament_match
    alter column external_link set not null;

comment on column tournament_match.external_link is
    'Where this match is recorded on another platform (the match import''s source URL, a bracket '
    'site, a VOD). Required, and unique within the tournament: it is what stops the same match '
    'being imported twice. A match with no such page carries a synthetic ''urn:umfl:match:<id>'' '
    'placeholder rather than a null.';
