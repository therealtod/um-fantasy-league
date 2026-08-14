-- ===========================================================================
-- `scoring_rule_set.is_active` defaults to false, not true.
--
-- The baseline defaulted it to true, which the application never noticed:
-- Spring Data JDBC writes every mapped column, so `ScoringRuleSet.isActive`
-- (false) always won and `AdminScoringService.create` then activated the row
-- through `activate` when asked. The default only ever applied to a
-- hand-written INSERT that omitted the column -- a fixture, a psql session, a
-- future migration -- and there it did the wrong thing twice over: it made a
-- draft rule set live, and if the tournament already had an active one it
-- tripped `uq_scoring_rule_set_active` with a constraint error rather than
-- writing the row.
--
-- Inactive is also what the domain means by "a rule set exists": at most one
-- of a tournament's rule sets may be active, and which one that is is an
-- explicit admin decision (`POST .../scoring-rule-sets/{id}/activate`), never
-- a property of having been inserted.
--
-- Forward-only, and it touches no data: existing `is_active` values are left
-- exactly as they are, since the default applies to future inserts only.
-- ===========================================================================

alter table scoring_rule_set
    alter column is_active set default false;
