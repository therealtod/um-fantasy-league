-- ===========================================================================
-- Demo roster roles.
--
-- Dev/test only, like the rest of `db/seed`. Winter of Champions is the
-- walkthrough tournament -- REGISTRATION_OPEN, no entries, no results -- so it
-- is the one that gets roles: an admin can drive the whole mechanic end to end
-- there, and Summer of Legends' recorded standings, which the integration
-- suite asserts exactly, are untouched.
--
-- The roles are defined but `roles_enabled` is left **off**. Winter is also
-- where the integration suite registers, drafts and locks rosters, and with
-- roles on every one of those locks would need a role per hero. A demo starts
-- by switching it on from the admin tournament form -- which is the toggle
-- doing exactly what it is for -- and the role tests switch it on themselves.
--
-- The stat names are illustrative. They are whatever the tournament's stats
-- sheet has columns for; nothing in the schema fixes them.
-- ===========================================================================

insert into roster_roles (tournament_id, name, max_per_roster, sort_order)
select t.id, r.name, r.max_per_roster, r.sort_order
from tournaments t
    cross join (values ('Attacker',  null::integer, 1),
                       ('Healer',    1,             2),
                       ('Tactician', null::integer, 3)) as r (name, max_per_roster, sort_order)
where t.name = 'Winter of Champions';

insert into roster_role_weights (role_id, stat, coefficient)
select rr.id, w.stat, w.coefficient
from roster_roles rr
    join tournaments t on t.id = rr.tournament_id
    join (values ('Attacker',  'ATTACKS',        1.0000),
                 ('Attacker',  'DAMAGE_DEALT',   0.2500),
                 ('Healer',    'HEALING',        1.5000),
                 ('Tactician', 'SCHEMES_PLAYED', 2.0000)) as w (role, stat, coefficient)
        on w.role = rr.name
where t.name = 'Winter of Champions';
