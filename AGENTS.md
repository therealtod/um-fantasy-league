# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A fantasy layer over **real** Unmatched tournaments: an admin records match results, managers draft
heroes under a per-tournament budget, and points are derived from the recorded results by a scoring
table that lives in the database. Nothing is simulated: match results, tournaments, heroes, maps and
scoring rules are real facts an admin enters through the Admin API (`/api/admin/...`), not computed
or randomly generated. Gradle multi-project (`:backend` only) + a separate npm frontend that is not
part of the Gradle build.

`README.md` carries the domain rationale (why Spring Data JDBC over JPA, why cost is per tournament,
why scoring is data, seed-data tuning). Read it before making design-level changes.

## Commands

```bash
# Database (Postgres 17 on host port 5433)
docker compose up -d db

# Backend — Flyway migrates and seeds on first start
./gradlew :backend:bootRun --args='--spring.profiles.active=dev'

# Frontend (separate shell) — Vite proxies /api to localhost:8080
cd frontend && npm install && npm run dev
```

Migrations are periodically squashed back into a single `V1__core_schema.sql` baseline (schema +
seed fixtures + admin authorization all in one file), so an older local database fails Flyway
checksum validation. There is no production data: `docker compose down -v && docker compose up -d
db`.

Tests:

```bash
# Pure domain logic, no Docker needed — the highest-value gate in the repo
./gradlew :backend:test --tests '*RosterPolicyTest' --tests '*ScoringEngineTest' --tests '*MatchMetricsTest'

# Single test class / method
./gradlew :backend:test --tests 'com.umfl.tournament.RosterPolicyTest'
./gradlew :backend:test --tests '*RosterPolicyTest.some test name'

# Full suite — Testcontainers integration tests need a running Docker daemon
./gradlew :backend:test

cd frontend && npm test              # vitest run, src/**/*.spec.ts
cd frontend && npm run type-check    # vue-tsc --noEmit
cd frontend && npx vitest run src/domain/rosterPolicy.spec.ts   # single file
```

Formatting:

```bash
./gradlew :backend:ktlintCheck    # merge gate, alongside :backend:test
./gradlew :backend:ktlintFormat   # auto-fix — read the diff, it is not always an improvement
```

Everything under `com.umfl.support.PostgresIntegrationTest` shares one static Postgres container and
runs inside a rolled-back transaction, so tests may mutate seed data freely. That base class
deliberately does **not** use `@Testcontainers`/`@Container`: the extension stops a static container
in `afterAll` of *every* class it annotates, so from the second test class onward Spring's cached
context points at a dead port. The container is started in a static initializer and lives for the
JVM. Don't add the annotations back.

ktlint runs off the root `.editorconfig`, on the `intellij_idea` style baseline rather than the
stricter `ktlint_official` one: the linter is there to *preserve* the existing formatting and catch
the mechanical slips (unused imports, import order, stray whitespace), not to reformat the codebase
into a different style. Three rules are off everywhere — the two `trailing-comma-*` rules and
`function-signature`/`class-signature` — because they contradict each other over this codebase's
multi-line-signature-with-trailing-comma house style, and `ktlintFormat` resolves that contradiction
by collapsing declarations onto one line with a dangling comma. Test sources additionally opt out of
the wrapping rules and the line-length cap so fixture tables can stay tabular. Retune the
`.editorconfig` rather than reformatting a file to satisfy a rule.

Spring Boot is pinned to 4.1.0 (managing Testcontainers 2.0.5, up from the 1.21.x line this repo
started on). The original reason for the pin — 1.21.3 negotiating a Docker API version too old for
Docker Engine 29+ — is moot now that the managed Testcontainers major version has moved past it;
verified working against Docker Engine 29.7.1 (API 1.55). Re-verify against your Docker Engine
version before downgrading Spring Boot back toward the 3.5.x/Testcontainers 1.21.x line.

## Profiles

| Profile | Auth | Notes |
|---|---|---|
| `dev` | `DevManagerAuthenticationFilter` resolves `X-Manager-Id` (falls back to seeded *NeonStrategist*) once at the filter level; `DevManagerProvider` just reads it back off `SecurityContextHolder` | Just DEBUG logging on top of the defaults |
| `test` | same dev stub | Testcontainers Postgres via `@ServiceConnection` |
| `prod` | `SupabaseAuthenticationConverter` verifies the Supabase JWT, resolves `sub` → `manager.auth_user_id`, JIT-provisions, once per request; `SupabaseManagerProvider` just reads the result back off `SecurityContextHolder` | Needs `DB_URL`/`DB_USER`/`DB_PASSWORD`/`SUPABASE_JWKS_URI`, plus optional `FRONTEND_ORIGIN` (only for a frontend calling the API cross-origin instead of through `_redirects`) |

There are no scheduled tasks and no background workers in any profile, with one narrow exception:
`StandingsSseHub` runs a single-thread keep-alive that pings open standings SSE connections every
20s so idle-timeout proxies/browsers don't silently drop them. It's transport plumbing for the live
standings feed below, not a business-logic worker — it has no DB access and does nothing but write
SSE comment lines to already-open connections.

`SecurityConfig` (prod) is a stateless resource server whose public-GET allowlist covers
`/api/tournaments`, `/api/tournaments/*`, `/api/tournaments/*/heroes`, `/api/tournaments/*/standings`,
`/api/tournaments/*/standings/stream` and `/api/tournaments/*/matches`; keep it in step with
`SecurityConfigTest`. Admin routes
(`/api/admin/**`) require `hasRole("ADMIN")` in both `SecurityConfig` and `DevSecurityConfig` — the
role comes from `manager.is_admin`, our own data, resolved once per request by
`SupabaseAuthenticationConverter`/`DevManagerAuthenticationFilter` via the shared, provider-agnostic
`ManagerAuthorities` — never from an identity-provider claim, so swapping auth providers later never
touches the role logic. `DevSecurityConfig` (`!prod`) is otherwise a deliberate permit-all chain that
exists only to stop Spring Security's autoconfiguration from securing everything the moment the
oauth2 starter is on the classpath. Don't delete it.

Those matcher lists are the first layer, not the only one: `MethodSecurityConfig` turns on
`@EnableMethodSecurity`, and every admin controller (plus `TournamentController.delete`, the one
admin operation living outside `/api/admin/**`) also carries `@PreAuthorize("hasRole('ADMIN')")`.
The annotation travels with the code, so a future admin endpoint that forgets a URL matcher is still
gated — two independent layers is the point, so keep both in step rather than collapsing one into
the other.

Adding a Supabase project also requires the Discord provider configured in the Supabase dashboard —
the backend never talks to Discord, only to Supabase-signed tokens.

## Backend architecture

Package-by-feature under `com.umfl`: `hero`, `manager`, `match`, `scoring`, `standings`,
`tournament`, plus `api` (controllers + DTOs), `auth`, `common`, `config`.

**The read/write split is the central convention.** Spring Data JDBC repositories handle writes and
aggregate loading; hand-written `JdbcClient` projections handle reads. Don't reach for a repository
derived-query method when the screen wants a joined projection, and don't add an ORM.

- Writes/aggregates: `TournamentEntryRepository`, `TournamentRepository`, `ManagerRepository`
- Reads: `HeroQueryRepository` (`HeroView`), `MatchResultQuery`, `ScoringRuleSetQuery`,
  `StandingsQuery`

`TournamentEntry` is the only aggregate root — it owns `EntrySlot`s via `@MappedCollection` (list
index → `entry_slot.slot_index`) and is saved as one unit. The only other write is `manager`, on
JIT-provisioning in `prod`. Everything else — `heroes`, `tournament`, `tournament_hero`, and the
match/scoring tables — is read-only to this application; those rows come from Flyway seed SQL.

### Invariants

- **No `season`, anywhere.** The tournament is the unit of scoping. Hero cost is
  `tournament_hero.cost`; queries take a `tournamentId`.
- **No cost snapshot.** `entry_slot` stores only the hero; cost is joined live, so re-pricing a hero
  re-prices an *unlocked* roster. That is intended. What *is* snapshotted is
  `tournament_entry.credit_grant`, copied off the tournament at registration —
  `RosterPolicy.validateLock` takes the budget from the entry, never the tournament.
- **Nothing writes points.** Match results are written by the Admin API (see below), but every
  point total is still derived at read time in `StandingsService`; `totalCost` is derived from
  slots. Do not materialise either.
- **There are no `umfl.*` configuration properties.** Scoring weights are rows in
  `scoring_coefficient`, the budget is `tournament.credit_grant` — both retuned with an UPDATE. Don't
  reintroduce a tunables block in `application.yml`.
- **No `player` entity.** Every point is scored per *hero*: no metric extractor, no coefficient and
  no standings query reads the human who piloted it. So the competitor is
  `match_participant.player_label` — nullable free text with no table, no FK, no repository and
  deliberately no admin API. An admin records a new competitor by typing their name. It is display
  text for the ticker and the admin match list, nothing more, and `MatchResultPolicy` never validates
  it (a blank label normalises to null in `AdminMatchService`). Promote it to a real table only if
  something starts scoring or ranking the humans — until then, a `player` table only buys you CRUD
  you have to build and a foreign key that can 500.

Domain rules live in pure `object`s with no Spring or persistence dependency — `RosterPolicy`,
`MatchMetrics`, `ScoringEngine`. New rules belong there, tested directly, not inside a `@Service`.

`RosterPolicy.validateDraft` deliberately permits over-budget selections (the builder is a
scratchpad, the meter just runs past 100%); `validateLock` adds the budget and roster-size checks.
Both return *all* violations at once so the UI can highlight every problem in one pass. Rule codes:
`ENTRY_LOCKED`, `TOURNAMENT_CLOSED`, `TOO_MANY_PICKS`, `INCOMPLETE_ROSTER`, `DUPLICATE_HERO`,
`UNKNOWN_HERO`, `BUDGET_EXCEEDED`.

`MatchMetrics` is a registry keyed by the free-form `scoring_coefficient.metric` string. It
implements `APPEARANCE`, `BAN`, `WIN`, `LOSS`, `DRAW`, `HEALTH_REMAINING`, `HEALTH_DIFFERENTIAL`,
`SHUTOUT`, and **silently ignores everything else** — unknown keys score zero, are dropped from the
leaderboard's columns and throw nothing. The seed's `CROWD_FAVOURITE` is the deliberate proof of
that; leave it unimplemented. Extractors take a `MetricContext` (the hero's role in one whole match),
not a bare participant row, because `HEALTH_DIFFERENTIAL` needs the opponent, `DRAW` needs "did
anyone win", and `BAN` has no participant row at all.

`StandingsService` returns a `StandingsBoard` that carries its own `metrics` column definitions —
the backend cannot know the columns until it reads `scoring_coefficient`. Ranking is standard
competition ranking (1, 2, 2, 4). The ticker's polling key is **`sinceMatchId`** (monotonic
`bigserial`), never `playedAt`: parallel tables in a round share a timestamp.

`GET /api/tournaments/{id}/standings/stream` is an SSE endpoint (`StandingsController` →
`StandingsSseHub`) that pushes a bare "something changed" event after `AdminMatchService.record` /
`correct` / `delete` commits (`StandingsUpdateEvent`, delivered via
`@TransactionalEventListener(phase = AFTER_COMMIT)` so it only fires once the row is actually
visible). The event carries no board/ticker payload — the frontend already knows how to pull fresh
data via the existing `/standings` and `/matches` endpoints, so the stream is purely a "poll now"
signal, not a second copy of the data.

Controllers take the caller via `@CurrentManager` (resolved by `CurrentManagerArgumentResolver`
against the `CurrentManagerProvider` seam); a nullable parameter yields `currentOrNull()`. Errors go
through `GlobalExceptionHandler` as RFC 7807 problem details — `NotFoundException` → 404,
`ConflictException` → 409, `RosterRuleException` → 422 with a `violations` array.

`HeroSort` holds ORDER BY fragments as an enum whitelist because sort keys can't be parameterised —
keep new sorts inside that enum.

Migrations are `backend/src/main/resources/db/migration/V*__*.sql`, forward-only. There is a single
`V1__core_schema.sql` baseline — schema, seed fixtures, and the admin-authorization column all
squashed into one file, periodically re-squashed the same way rather than left as an accumulating
chain, since there is no production data yet to preserve migration history for. Tables the app loads
or writes as aggregates carry a surrogate `bigserial` id next to a unique natural key because Spring
Data JDBC can't map composite primary keys; the pure link tables (`tournament_hero`, `tournament_map`,
`entry_slot`, `match_ban`) keep natural composite keys. The seed fixture is still the original seed,
and the integration tests assert on its numbers exactly — changing a seeded price or result means
updating them. New data past that fixed baseline goes through the Admin API (see below), not another
migration.

## Frontend architecture

Vue 3 `<script setup>` + Pinia setup-stores + Vue Router, Tailwind v4. `@/` aliases `frontend/src`.

`src/api/client.ts` is the only place that calls `fetch` — it attaches the Supabase bearer token,
unwraps RFC 7807 bodies into `ApiError` (with `.violations` for 422s), and exposes a typed `api`
object. Add endpoints there, with matching types in `src/api/types.ts` — that file is the contract
anchor, so change it first and let `npm run type-check` point at every consumer. Jackson runs with
`default-property-inclusion: non_null`, so nullable fields are *absent*, not `null`; templates need a
`?? '—'` fallback (e.g. `mapName`).

Stores: `auth` (Supabase session), `manager`, `heroes` (keyed by tournament — cost is
tournament-scoped), `tournaments`, `roster`, `standings`. `roster` keeps an optimistic `selectedIds`
and rolls it back if the server rejects. `standings` loads once via `load(id)`, then opens the
`/standings/stream` SSE connection (`src/api/sseClient.ts`) and calls `refresh()` — the store's
existing incremental fetch, keyed on `highestMatchId` — every time the backend signals that a match
was written. There's still no client-side polling timer; the server-pushed event is what triggers
each `refresh()`. The stream is closed and reopened on tournament switch and closed on unmount.

`src/domain/rosterPolicy.ts` intentionally duplicates the Kotlin budget arithmetic (`budgetStatus`)
so the meter reacts on click. **If you change that arithmetic, change both sides** —
`RosterPolicy.kt` and `rosterPolicy.ts` — and their tests. The server stays authoritative.

Routes: `/lobby`, `/standings` and `/login` are public; `/tournaments/:tournamentId/roster` requires
a session plus a `beforeEnter` guard that bounces to the lobby unless the manager has an entry or
the tournament accepts registration. `/admin` (`AdminDashboardView.vue`) has a `beforeEnter` guard
that bounces non-admins to the lobby, and `AppShell.vue` only renders the nav link when
`manager.isAdmin` — both are UI convenience, not the security boundary, since every Admin API call is
still `hasRole("ADMIN")`-gated server-side.

The UI implements the Stitch "Tactical Analytics" design system: obsidian surfaces, 1px borders,
**zero border radius everywhere** (`main.css` sets `* { border-radius: 0 }` as a base rule), neon
cyan/magenta/lime reserved for live data. Design tokens are Tailwind v4 `@theme` variables in
`src/assets/main.css` — use `bg-surface-low`, `text-ink-dim`, `border-edge`, `font-display` rather
than hex values or stock Tailwind grays.

Layout is **mobile-first**: unprefixed classes are the phone layout and prefixes add desktop, on the
stock Tailwind breakpoint scale — there are deliberately no `--breakpoint-*` overrides in `@theme`.
Two hinges carry almost all of it. **`md:` (768px)** is the shell hinge: below it `AppShell`'s 224px
rail becomes an off-canvas drawer (`fixed` + `invisible`/`-translate-x-full`, toggled by `navOpen`),
and `main` padding steps `p-4` → `p-8`. `invisible` is load-bearing — translating alone would leave
the closed drawer's links in the tab order. **`lg:` (1024px)** is where `RosterBuilderView`'s 320px
`RosterUplinkPanel` moves back alongside the hero grid; below it the panel stacks under the grid and
a second `BudgetMeter` rides `sticky top-0` above the pool so the budget stays visible while picking.

The standings leaderboard scrolls horizontally rather than shrinking, since its column count comes
from the active rule set and isn't knowable in advance. Rank and Manager are pinned via `.cell-pinned`
(`main.css`), which needs an *opaque* row underneath it — hence `.row-opaque` / `.row-opaque-mine`
instead of the translucent tint a highlighted row would otherwise use. Two constraints there are easy
to break: the pinned seam is an inset `box-shadow` (`.cell-pinned-edge`) because under
`border-collapse` the borders belong to the table and don't travel with a sticky cell, and the
Manager column's `left-12` offset only lines up while the Rank column's content plus `px-3` padding
stays under its declared `w-12`.

`e2e/responsive.spec.ts` guards all of this from the `mobile` Playwright project (Pixel 5): it
asserts `document.documentElement.scrollWidth` never exceeds the viewport on any route, which is the
failure mode that matters — nothing sets `overflow-x` on `body`, so one over-wide element slides the
whole page. Like the rest of the e2e suite it needs a live backend and is not part of CI.

`frontend/.env.local` needs `VITE_SUPABASE_URL` and `VITE_SUPABASE_ANON_KEY` (see `.env.local.example`),
plus optional `VITE_DEV_MANAGER_ID` to skip Supabase Auth against a dev backend.

## Admin API

`/api/admin/**`, `hasRole("ADMIN")`-gated, backed by `manager.is_admin` (our own data, independent of
any identity provider). Covers create/update for tournaments, heroes, maps, per-tournament hero
pool/pricing (`tournament_hero`), per-tournament board pool (`tournament_map`), and scoring rule
sets/coefficients, plus create/update/delete for match results. Both pools also support removal, and
the two removals are deliberately asymmetric: dropping a hero from `tournament_hero` is always
allowed and simply re-prices any roster still holding it to 0 (the "no cost snapshot" invariant
above, applied to a removal rather than a re-price), while dropping a map from `tournament_map` is
rejected with a `ConflictException` when the tournament has a recorded match on it, since
`tournament_match` carries a composite FK onto that row. The `V1__core_schema.sql` seed SQL is
still what populated the original fixtures — nothing about it changed — but it is no longer the only
way new reference data or results can enter the system.

Tables that were previously read-only (`heroes`, `game_map`, `tournament_match`/`match_participant`/
`match_ban`, `scoring_rule_set`/`scoring_coefficient`) now have a Spring Data JDBC write side:
`Hero`, `GameMap`, `TournamentMatch` (owning `participants`/`bans` as `Set`-mapped children — no
`keyColumn`, unlike `EntrySlot`'s `List`, because neither child has a list-position column) and
`ScoringRuleSet` (owning `coefficients`). `tournament_hero` and `tournament_map` stay composite-keyed
link tables with no Kotlin entity — `HeroPoolAdminRepository`/`MapPoolAdminRepository` write them via
`JdbcClient`, the same read/write split the rest of the app already uses.

`MatchResultPolicy` (pure, mirrors `RosterPolicy`) validates a match submission — map-in-pool,
duplicate hero, unknown hero, at most one winner — before save, raising `MatchRuleException` (422, same
shape as `RosterRuleException`, kept as a separate type rather than merged into it). Activating a
scoring rule set deactivates any active sibling in the same transaction, since only one may be active
per tournament. An unknown scoring metric (e.g. the seed's `CROWD_FAVOURITE`) is surfaced as a
non-blocking warning on the response, never rejected.

`ScoringRuleSetPolicy` (pure, same pattern again) validates a rule set's coefficients on create and
update, with rule codes `DUPLICATE_METRIC` and `MALFORMED_METRIC`, raising `ScoringRuleException` —
a third 422 type of the same shape. Both checks run against the *normalised* metric, so `' win '`
and `'Win'` are a duplicate of each other, and `MALFORMED_METRIC` mirrors the schema's
`^[A-Z][A-Z0-9_]*$` CHECK in Kotlin. Without it a duplicated or hyphenated metric reached the
database and came back as the `DataIntegrityViolationException` backstop's generic 409, which names
nothing. The policy validates the *shape* of a metric name and never the *set* — an unimplemented
metric stays a warning, per the paragraph above.

## Admin frontend

`/admin` (`AdminDashboardView.vue`) is a manager-gated dashboard, not a separate app — it composes
per-entity wizard components (`TournamentManagementWizard`, `HeroManagementWizard`,
`MapManagementWizard`, `HeroPoolWizard`, `MapPoolWizard`, `ScoringRuleSetWizard`,
`MatchResultWizard`, `MatchListAdmin`) that each call the corresponding `/api/admin/...` endpoints
through the same `src/api/client.ts`. It's reachable only by managers with `isAdmin` true — see
Routes above for the two layers of UI gating — with the Admin API's own `hasRole("ADMIN")` check as
the actual security boundary.

`ScoringRuleSetWizard` is the one place the "unknown metrics are a warning, not a rejection" rule
becomes visible. It lists a tournament's rule sets with their active flag, edits and activates them,
and **renders `ScoringRuleSetDto.warnings` after every save** — without that, a mistyped metric is a
clean `201` followed by a column that silently scores zero forever. Its `knownMetrics` array mirrors
`MatchMetrics`' extractor registry and only drives a hint (an inline flag while typing, normalised
the same `trim().uppercase()` way); it never blocks a save, because the server deciding what it can
price is the whole point of the warning. Keep the array in step when adding an extractor.

## Deliberately not built

A Hero Encyclopedia / Stats Lab (third-party sites already publish Unmatched stats — `heroes` is only
`(id, name, image_url)` on purpose).

## CI/CD

GitHub Actions. `.github/workflows/backend-ci.yml` runs `:backend:ktlintCheck` then `:backend:test` (Testcontainers
included — GitHub-hosted runners have Docker preinstalled) on PRs and pushes to non-`master`
branches, as the merge gate. `.github/workflows/backend-deploy.yml` re-runs the same tests, then on a green push to
`master` builds the root `Dockerfile`, pushes `ghcr.io/<owner>/umfl-backend:{sha,latest}`, and SSHes
into the VPS to `docker compose pull && up -d` against `deploy/docker-compose.prod.yml` (which the
VPS keeps a copy of at `/opt/umfl`, alongside a `.env` — modeled on `deploy/.env.example` — that is
managed by hand on the box, never passed through CI). The `prod` profile there talks to Supabase
Postgres, so there is no `db` service in that compose file, unlike the root `docker-compose.yml`.
`.github/workflows/frontend-ci.yml` runs on `frontend/**` changes — `npm ci`, `npm run type-check`,
`npm test` (vitest) — as that side's merge gate; it does not build or deploy anything. Deployment of
the frontend stays separate: Cloudflare Pages is connected directly to the GitHub repo and builds
`frontend/` on every push and PR (its own preview-deployment mechanism, running `vue-tsc -b && vite
build`, no tests), independent of the backend pipeline — the two sides deploy independently since
neither needs the other to be green. The Playwright e2e suite (`frontend/e2e/`) needs a live backend
and Postgres and is not wired into either workflow yet.
`frontend/public/_redirects` is what routes the deployed frontend to the backend: a
`/api/* https://<BACKEND_HOST>/api/:splat 200` rule that Cloudflare Pages applies so the frontend's
relative `/api/...` calls (`client.ts`, `sseClient.ts`) reach the VPS as a same-origin proxy, with no
cross-origin request involved. `<BACKEND_HOST>` is a placeholder — fill it in with the real production
hostname before this is live.
