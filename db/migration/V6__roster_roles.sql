-- ===========================================================================
-- Roster roles, and the per-game stats they price.
--
-- Until now a hero scored the same for every manager who held it: every point
-- came off the match result, and the match result does not know who drafted
-- the hero into a fantasy roster. This adds a layer the manager controls. An
-- admin defines a tournament's roles ("Attacker", "Healer"), each weighting a
-- handful of in-game stats; a manager gives every hero on their roster a role;
-- and an admin records, alongside a match, how often each hero attacked,
-- healed and so on in each game. The role bonus is those stats priced by the
-- role *this manager* chose.
--
-- As everywhere else in this schema, nothing below stores a point. The bonus
-- is folded at read time by `umfl_domain::standings::board`, so retuning a
-- role's weights is a bare UPDATE that the next standings read reflects.
--
-- The whole mechanic is switched per tournament by `roles_enabled`, which
-- every existing tournament gets as false: with it off, the board, the roster
-- builder and the match wizard behave exactly as they did before this file.
-- ===========================================================================

alter table tournaments
    add column roles_enabled boolean not null default false;

comment on column tournaments.roles_enabled is
    'Whether managers assign roles to their heroes and the standings price the role bonus. '
    'Switching it off hides the bonus without deleting anything, so switching it back on restores it.';

-- One role in one tournament. Per tournament rather than global for the same
-- reason a hero's price is: the tournament is the unit of scoping, and the
-- weights that make a role worth taking are a balancing decision about one
-- event.
--
-- `max_per_roster` is an optional cap ("at most one Healer"), checked when a
-- roster locks and whenever a locked roster's roles change. Null is uncapped.
--
-- `sort_order` fixes the order roles are offered in, which nothing else could
-- determine.
create table roster_roles (
    id             bigserial primary key,
    tournament_id  bigint  not null references tournaments (id) on delete cascade,
    name           text    not null,
    max_per_roster integer check (max_per_roster > 0),
    sort_order     integer not null default 0,
    unique (tournament_id, name)
);

-- One weighted stat on one role. Shaped exactly like `scoring_coefficients`:
-- `stat` is free-form text with the same SCREAMING_SNAKE typo guard rather
-- than an enum, because the stats a tournament collects are whatever its
-- stats sheet has columns for, and an admin must be able to price a new one
-- without a migration. A stat no recorded game carries simply scores zero.
--
-- No check on `coefficient`: a negative weight is a legitimate penalty.
create table roster_role_weights (
    role_id     bigint         not null references roster_roles (id) on delete cascade,
    stat        text           not null,
    coefficient numeric(10, 4) not null,
    primary key (role_id, stat),
    constraint roster_role_weight_stat_format
        check (stat ~ '^[A-Z][A-Z0-9_]*$')
);

-- Which role one entry gives one hero, from one round on.
--
-- An event log rather than a `role_id` column on `entry_slots`, for the same
-- reason `roster_swaps` is a log: points stay in the round they were earned
-- in. A manager may re-assign a role during a swap window, and without the
-- history a re-assignment in round 3 would re-price rounds 1 and 2 under a
-- role the hero did not have when it played them. The role in force for
-- (entry, hero, round) is the row with the greatest `from_round` not past it.
--
-- While an entry is still a draft every row sits at `from_round = 1` and is
-- simply replaced on each edit -- there is no history yet worth keeping. Once
-- locked, a change is written at the tournament's current round.
--
-- `hero_id` is deliberately not a reference to `entry_slots`: a hero swapped
-- out loses its slot, but the rounds it played under this role still need
-- pricing. The hero FK has no cascade, matching `roster_swaps`.
--
-- `role_id` has no cascade either: deleting a role somebody has assigned
-- would silently re-price their past rounds, so it fails loudly instead, and
-- `role::admin_service::delete` checks first so the admin gets a message that
-- names the problem. That the role belongs to the entry's own tournament is
-- an application-level rule (`RosterRule::UnknownRole`), the same tier of
-- guarantee as "a roster hero is in this tournament's pool".
create table entry_hero_roles (
    entry_id   bigint  not null references tournament_entries (id) on delete cascade,
    hero_id    bigint  not null references heroes (id),
    from_round integer not null check (from_round > 0),
    role_id    bigint  not null references roster_roles (id),
    primary key (entry_id, hero_id, from_round)
);

create index idx_entry_hero_role_role on entry_hero_roles (role_id);

-- One stat for one side of one game: how often that hero attacked, healed,
-- schemed. A fact about the match, in the same category as
-- `health_remaining`, so it is written only by `r#match::writer` inside the
-- match's own transaction -- which is what keeps `MatchResultCache`'s
-- invalidation complete without a new hook.
--
-- Keyed by (game, side) and referencing `match_game_participants`, so a stat
-- can only belong to a hero that actually played that game, and is cascaded
-- away with it when a correction rewrites the games.
--
-- `value` is a count, so it is a non-negative integer.
create table match_game_stats (
    game_id bigint  not null,
    side    integer not null,
    stat    text    not null,
    value   integer not null check (value >= 0),
    primary key (game_id, side, stat),
    constraint match_game_stat_of_participant
        foreign key (game_id, side) references match_game_participants (game_id, side)
        on delete cascade,
    constraint match_game_stat_format
        check (stat ~ '^[A-Z][A-Z0-9_]*$')
);
