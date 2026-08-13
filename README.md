# UM Fantasy League

A fantasy league layered over **real** Unmatched tournaments: an admin records who played which
hero and how the match ended, managers draft a roster of heroes under a per-tournament budget, and
every recorded result is priced into fantasy points by a scoring table the admin controls.

Nothing here is simulated. The application has no writer for match results — results are facts,
points are derived from them.

The UI follows the "Tactical Analytics / Mission Control" design system authored in Google Stitch
(project `13163612977730740340`) — obsidian surfaces, 1px technical borders, zero corner radius,
neon accents reserved for live data.

---

## Stack

| Layer | Choice |
|---|---|
| Language / build | Kotlin 2.3, JVM 21, Gradle Kotlin DSL (wrapper 8.14.3) |
| Backend | Spring Boot 4.1 — web, validation, actuator |
| Persistence | PostgreSQL 17 + Spring Data JDBC + Flyway |
| Frontend | Vue 3.5 + TypeScript + Vite 7 + Pinia + Vue Router + Tailwind CSS v4 |
| Tests | JUnit 5 (+ Testcontainers), Vitest |

### Why Spring Data JDBC rather than JPA/Hibernate

- Kotlin `data class` entities work natively — no `no-arg`/`all-open` compiler plugins, no open
  classes, no mutable-field compromises.
- No lazy-loading proxies or N+1 surprises on the aggregation-heavy Standings screen. Every query
  is explicit.
- Aggregate boundaries fit the domain: `TournamentEntry` genuinely owns its `EntrySlot`s and is
  saved as one unit. Almost nothing else in the schema is written by the app at all.
- `JdbcClient` is available for hand-written analytics SQL (leaderboards, ticker feeds) without
  fighting an ORM.

The codebase leans into that split: **Spring Data JDBC repositories for writes and aggregates,
`JdbcClient` projections for reads** (`HeroQueryRepository`, `MatchResultQuery`,
`ScoringRuleSetQuery`, `StandingsQuery`).

---

## Running it

Prerequisites: JDK 21, Node 20+, Docker.

```bash
# 1. Database
docker compose up -d db

# 2. Backend — migrates and seeds on first start
./gradlew :backend:bootRun --args='--spring.profiles.active=dev'

# 3. Frontend (separate shell)
cd frontend && npm install && npm run dev
```

Then open <http://localhost:5173>. The Vite dev server proxies `/api` to `localhost:8080`.

> **Pulling this schema onto an existing local database?** The migrations were rewritten in place as
> a fresh baseline, so Flyway's checksum validation will fail against a database migrated from the
> old files. There is no production data to preserve — drop the volume and start over:
>
> ```bash
> docker compose down -v && docker compose up -d db
> ```

`V1__core_schema.sql` is schema only — no mock data. The three demo tournaments, the seeded
managers, and the recorded Summer of Legends result set are a second migration,
`db/seed/V2__demo_fixtures.sql`, that only the `dev` and `test` profiles add to
`spring.flyway.locations`. A plain start with no profile, or `--spring.profiles.active=prod`, migrates
schema only and boots with an empty database — nothing to delete before pointing this at a real
tournament.

For the local workflow, set `VITE_DEV_MANAGER_ID=1` in `frontend/.env.local` (the supplied example
does this). Vite development builds then skip Supabase Auth and send that manager ID to the dev
backend. Seeded manager IDs are 1 through 4; change the value and restart Vite to exercise another
manager. This mode is unavailable in production builds and the `X-Manager-Id` header is accepted
only by the non-production backend profile.

Setting it is effectively required for anything past the public screens. Without it the frontend
sends a Supabase bearer token instead, which the dev backend does not read, so the lobby and
standings still load but the roster and admin screens answer 401 — a dev backend treats a request
with no `X-Manager-Id` as anonymous rather than guessing at an identity for it.

There is no background process. The Standings screen loads once, because nothing writes results
while the app is running.

### Running against Supabase (`prod` profile)

Requires a Supabase project with the Discord provider configured (Authentication → Providers →
Discord in the Supabase dashboard, backed by a Discord Developer Portal application) plus:

| Env var | Purpose |
|---|---|
| `DB_URL` | Supabase Postgres connection string — use the **direct connection** or **Session pooler** (not the Transaction-mode pooler on port 6543, which breaks JDBC prepared-statement caching); append `?sslmode=require`. Database name is `postgres`, not `umfl`. |
| `DB_USER` / `DB_PASSWORD` | Supabase Postgres credentials |
| `SUPABASE_JWKS_URI` | `https://<project-ref>.supabase.co/auth/v1/.well-known/jwks.json` — only if your project uses modern JWKS signing keys (Project Settings → API → JWT Keys). Legacy HS256-secret projects need a `SUPABASE_JWT_SECRET`-driven `JwtDecoder` bean instead; see `SecurityConfig.kt`. |

```bash
DB_URL=... DB_USER=... DB_PASSWORD=... SUPABASE_JWKS_URI=... \
  ./gradlew :backend:bootRun --args='--spring.profiles.active=prod'
```

The frontend needs `frontend/.env.local` with `VITE_SUPABASE_URL` and `VITE_SUPABASE_ANON_KEY` (see
`frontend/.env.local.example`) — no profile flag needed there, it always talks to whichever backend
the Vite proxy points at.

### Tests

```bash
# Pure domain logic — no Docker required
./gradlew :backend:test --tests '*RosterPolicyTest' \
                        --tests '*ScoringEngineTest' \
                        --tests '*MatchMetricsTest'

# Everything, including Testcontainers integration tests (needs a running Docker daemon)
./gradlew :backend:test

# Frontend
cd frontend && npm test && npm run type-check
```

---

## Screens

| Route | Screen |
|---|---|
| `/lobby` | **Tournament Lobby** — format, dates, credit grant, enrolment, register |
| `/tournaments/{id}/roster` | **Roster Builder** — the tournament's hero pool with its prices, search/sort, live budget meter, lock |
| `/standings` | **Standings** — leaderboard with data-driven metric columns, plus a ticker of recorded matches |
| `/login` | **Sign in** — Discord via Supabase Auth |

Only the Roster Builder requires a session; the lobby and standings are public. It additionally
guards on entry: the route bounces back to the lobby unless the manager already has an entry or the
tournament is open for registration.

The Stitch project also contains a **Hero Encyclopedia / Stats Lab**. It is deliberately **not**
implemented: free third-party platforms already publish Unmatched hero statistics, so `heroes` here is
only an identity — `(id, name, image_url)` — and everything that varies hangs off the tournament that
decided it.

---

## Domain model

Everything hangs off the tournament, because the tournament is what decides it:

```
tournament ──< tournament_hero   >── hero            the pool, and its price here
           ──< tournament_map    >── game_map        the legal boards
           ──< scoring_rule_set  ──< scoring_coefficient
           ──< tournament_match  ──< match_participant              the two humans, per series
                                 ├──< match_game      >── game_map  one game, one board
                                 │         └──< match_game_participant >── hero
                                 └──< hero_ban        >── hero       struck once per series
           ──< tournament_entry  ──< entry_slot       >── hero
                     │
                  manager
```

### Why cost is per tournament

`tournament_hero.cost`, not `hero.cost`. The same hero is a bargain at one event and a premium pick
at the next — pricing is a lever the organiser pulls per tournament, which is exactly the knob that
makes drafting interesting. It also makes `UNKNOWN_HERO` a real rule rather than an existence check:
a hero absent from `tournament_hero` is simply not legal here.

There is deliberately **no** `season` column anywhere. The tournament is the unit of scoping; a
season is just a word someone puts in a tournament's name.

### Why the budget is per registration

`tournament_entry.credit_grant`, not a `manager.credits` wallet. A wallet made entering a tournament
a spending decision and let a manager be locked out of the game by a balance, which is the wrong
tension for a fantasy league. Registration is free; what it hands you is a budget for *this*
tournament.

The grant is snapshotted onto the entry, because retuning a tournament's grant afterwards must not
silently hand extra budget to managers who already drafted, nor retroactively invalidate them.

### Why there is no cost snapshot on the slot

`entry_slot` stores only the hero. Roster cost is joined live from `tournament_hero`, so **re-pricing
a hero re-prices every unlocked roster holding it** — intended, not a bug. A draft is a live
scratchpad against live prices; the thing that needs stability is the grant, and that *is*
snapshotted. Once an entry is LOCKED its slots can no longer change, so the only rosters a repricing
can move are the ones still being edited.

Total cost and total points are likewise never stored — they are derived from the slots and the
matches that produce them, so they cannot drift.

### Why the ban is its own table

`hero_ban`, not a `was_banned` flag on a participant row. A banned hero has no health and no
result. Storing it as a participant forces `health_remaining = 0`, which is indistinguishable
from "played and was defeated" and silently poisons every `HEALTH_REMAINING` sum and `SHUTOUT` check.

The ban hangs off the match, not off a game: a hero is struck once for the whole series, so it must
not be multiplied by the number of games played (`MatchResult.heroContexts` yields exactly one
`Banned` context per banned hero regardless of series length).

Bans are per match, so they never touch `entry_slot`: a hero banned in one round can be played in
the next, and its manager still scores the `BAN` coefficient for the round it sat out.

### Schema notes

- `game_map` and `tournament_match`, not `map` and `match` — both are reserved words in the SQL
  standard, and quoting them at every call site would be noise.
- Tables the app loads or writes as aggregates carry a surrogate `bigserial` id, because Spring Data
  JDBC cannot map a composite primary key. The pure link tables (`tournament_hero`, `tournament_map`,
  `entry_slot`, `hero_ban`, `match_game_participant`) are read through `JdbcClient` or mapped as
  `@MappedCollection` children, so they keep their natural composite key.
- A match is a **series**: the board moved from the match to `match_game.map_id` (not null — each
  game names its board), constrained by a composite FK onto `tournament_map`, so a game can only be
  played on a board that tournament uses. `match_game` denormalizes `tournament_id` to carry that
  FK, and a second composite FK pins the copy to its parent match's tournament.
- Exactly one side of a game carries `is_winner`. A partial unique index stops two; `MatchResultPolicy`
  stops zero. **There is no draw** — a losing hero always finishes on 0 or less health, while a winner
  may finish on any value, including a negative one.
- Matches are seeded in chronological order, so `id` order and `played_at` order agree. That is what
  lets the ticker poll on `id > :sinceMatchId` — `played_at` is **not** unique, because parallel
  tables in a round share a start time.

### Roster rules (`RosterPolicy`)

Pure functions, no Spring, no persistence — see `RosterPolicyTest`.

| Rule | Draft | Lock |
|---|---|---|
| Entry not already locked | ✅ | ✅ |
| Tournament still accepts changes | ✅ | ✅ |
| No duplicate heroes (`DUPLICATE_HERO`) | ✅ | ✅ |
| No more than `rosterSize` picks (`TOO_MANY_PICKS`) | ✅ | ✅ |
| Exactly `rosterSize` picks (`INCOMPLETE_ROSTER`) | — | ✅ |
| Within the entry's credit grant (`BUDGET_EXCEEDED`) | — | ✅ |

`UNKNOWN_HERO` is raised earlier, in `TournamentService.resolvePicks`: a pick is priced by joining
`tournament_hero`, so a hero outside this tournament's pool never reaches the policy at all.

Going over budget is allowed **while drafting** — a draft is a scratchpad, and the builder shows a
meter running past 100% rather than refusing the edit. The budget is enforced when locking, against
the grant on the *entry*.

`BudgetStatus` is plain arithmetic — `spent`, `creditGrant`, `remaining` (negative when over) and
`utilisation`. There are no bands or thresholds: "how full is the bar" is the whole question the
meter answers. `frontend/src/domain/rosterPolicy.ts` mirrors that arithmetic so the meter responds on
click; the server recomputes it and rejects invalid locks with `422`.

---

## Scoring

**Scoring is data, not code.** A rule set is a row in `scoring_rule_set` (one active per tournament,
enforced by a partial unique index) and its weights are rows in `scoring_coefficient`. An admin
retunes the league with an `UPDATE`, not a redeploy.

`scoring_coefficient.metric` is free-form text — not an enum, not a foreign key — so adding a
weighted metric needs no migration. The `CHECK` on it is a typo guard (`SCREAMING_SNAKE` only), not a
whitelist. `com.umfl.scoring.MatchMetrics` is the registry that prices the keys this build
implements:

| Metric | Measures |
|---|---|
| `APPEARANCE` | the hero played this game |
| `BAN` | the hero was banned out of this match |
| `WIN` / `LOSS` | took / dropped this game — exhaustive, since every game has a winner |
| `HEALTH_REMAINING` | health at the end (0 if defeated) |
| `HEALTH_DIFFERENTIAL` | this hero's health minus the healthiest opponent's |
| `SHUTOUT` | every opponent finished on zero |

Anything else **contributes nothing, is dropped from the leaderboard's columns, and throws nothing**.
The seed ships a deliberately unimplemented `CROWD_FAVOURITE` weighted at 5.0 as standing proof of
that; do not implement it.

`HEALTH_DIFFERENTIAL` is symmetric in a two-sided match — if one side is +4 the other is −4 — so the
coefficient rewards a clean victory by exactly as much as it penalises a heavy defeat, and neither
creates nor destroys points across the match as a whole. There is deliberately **no** `DAMAGE_DEALT`:
`heroes` carries no starting-health column, so damage is not derivable from `health_remaining`. Adding
it is a schema decision, not a registry one. There is likewise **no** `DRAW`: a game with no winner
is not a recordable result, so a `DRAW` column would price something that cannot happen — it is
reported as an unknown metric like any other key this build cannot measure.

Metrics are measured per *game*, not per series: a hero that takes game 1 and drops game 2 of a Bo3
collects one `WIN` and one `LOSS`, two `APPEARANCE`s, and each game's own health numbers. The one
exception is `BAN`, which is struck once for the whole match.

Each metric's contribution is rounded to 2dp *before* it is summed, so a displayed total is exactly
the sum of its displayed breakdown. Coefficients may be negative — a penalty is a legitimate rule.

### Why points are derived, never stored

Coefficients are mutable reference data. A stored fantasy total would be a cache with nothing to
invalidate it: the moment an admin retunes a weight, every persisted total is silently wrong. There
is also no write path to maintain one — materialising points would push the formula into the seed
SQL, duplicating it.

The cost is negligible. `StandingsService` prices each `(hero, match)` pair exactly once and then
folds it into every roster holding that hero, which is cheaper than the equivalent SQL join's
fan-out, and at tournament scale (~50 matches) the whole fold is microseconds.

Consequences worth knowing:

- The leaderboard response is a `StandingsBoard` carrying its **own column definitions**, because the
  backend cannot know which columns exist until it has read `scoring_coefficient`.
- Ranking is **standard competition ranking** (1, 2, 2, 4). On a finished tournament, two managers
  with overlapping rosters genuinely tie, so positional `index + 1` would lie.
- `roundPoints` ("Last Rd") is the swing in `max(round)`.

---

## API

```
GET  /api/me                                     manager identity
GET  /api/tournaments                            ?status=
GET  /api/tournaments/{id}
GET  /api/tournaments/{id}/heroes                ?search= &sort=COST|NAME   this pool, these prices
POST /api/tournaments/{id}/entries               register — grants the tournament's credit_grant
GET  /api/tournaments/{id}/entries/me            roster + budget status
PUT  /api/tournaments/{id}/entries/me/slots      { "heroIds": [...] }
POST /api/tournaments/{id}/entries/me/lock       commit roster
GET  /api/tournaments/{id}/standings             StandingsBoard — metric columns + ranked rows
GET  /api/tournaments/{id}/matches               ?sinceMatchId= &limit=   ticker feed
```

The hero pool lives under the tournament because cost is tournament-scoped — a bare `/api/heroes`
could not answer the Roster Builder's actual question, which is "what can I pick here, and what does
it cost me".

Errors are RFC 7807 problem details. Roster rule breaches return `422` with every violation listed at
once, so the UI can highlight all of them rather than revealing one per attempt.

In `prod`, browsing is public and only your own entry needs an account: the tournament list, a single
tournament, its hero pool, its standings and its match feed are all permitted anonymously; `/api/me`
and everything under `/entries` require a verified token.

---

## Admin API

`/api/admin/**`, gated by `hasRole("ADMIN")` (backed by `manager.is_admin`, our own data — never an
identity-provider claim). This is the write path for everything that used to be seed-only: tournaments,
heroes, maps, a tournament's hero pool and pricing, its board pool, scoring rule sets and coefficients,
and match results (create/update/delete). `/admin` in the frontend is the UI over it.

`MatchResultPolicy` validates a match submission before save (each game's map in the tournament's
pool, no duplicate or unknown hero, dense 1..N game numbers, two sides per game, and exactly one
winner per game — zero is as invalid as two), returning `422` with every violation on failure, the
same shape as a roster rule breach. Activating a scoring rule set deactivates any active sibling for
that tournament in the same transaction — only one rule set may be active per tournament. Submitting
an unimplemented metric (e.g. `CROWD_FAVOURITE`) is a non-blocking warning on the response, not a
rejection.

---

## Authentication

`CurrentManagerProvider` is the seam, with two implementations selected by Spring profile:

- **`dev` / `test` / no profile** — `DevManagerAuthenticationFilter` resolves an `X-Manager-Id`
  header into the manager, and `DevManagerProvider` reads it back off the security context. NOT
  SUITABLE FOR ANY DEPLOYED ENVIRONMENT.

  A request with no header is simply anonymous — the same thing a request with no bearer token is
  under `prod` — so public routes serve it without touching the database and gated routes answer
  401. A header that is present but names no manager, or is not a number at all, is a 401 rather
  than a silent downgrade to anonymous.

  The Vite frontend can supply this header automatically when its development-only
  `VITE_DEV_MANAGER_ID` setting is present; this skips Discord so local end-to-end workflow tests
  do not need a Supabase project.

  ```bash
  curl -s localhost:8080/api/me -H 'X-Manager-Id: 3'
  curl -is localhost:8080/api/me | head -1          # 401: no header, no identity
  curl -is localhost:8080/api/tournaments | head -1 # 200: public either way
  ```

- **`prod`** — `SupabaseManagerProvider` verifies a Supabase-issued JWT (Spring Security's OAuth2
  resource server) and resolves the manager by the token's `sub` claim against
  `manager.auth_user_id`, just-in-time provisioning a new manager on first login.
  Sign-in happens via Supabase Auth's Discord OAuth provider from the frontend
  (`supabase.auth.signInWithOAuth({ provider: 'discord' })`) — Discord is just the upstream identity
  provider; the backend only ever verifies Supabase's own signed token.

Controllers depend only on the `CurrentManagerProvider` interface, so the two implementations are a
drop-in swap per profile — no other code changed.

---

## Not built yet

- Hero Encyclopedia / Stats Lab — third-party sites already publish Unmatched statistics.
- Production hosting/CI (the `prod` profile covers database and auth config; deployment itself is
  not automated).
