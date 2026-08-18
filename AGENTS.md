# AGENTS.md

Guidance for coding agents working in this repository. `CLAUDE.md` just imports this file.

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

# Backend — Flyway migrates the schema and, in dev/test only, seeds demo fixtures on first start
./gradlew :backend:bootRun --args='--spring.profiles.active=dev'

# Frontend (separate shell) — Vite proxies /api to localhost:8080
cd frontend && npm install && npm run dev
```

Migrations are periodically squashed back into a single `V1__core_schema.sql` baseline (schema +
admin authorization, no data), so an older local database fails Flyway checksum validation. There is
no production data: `docker compose down -v && docker compose up -d db`. Data past that baseline
splits by *kind*, not by environment: `db/migration/V2__reference_data.sql` carries the canonical
hero and board catalogue — facts about Unmatched, not about a league, and the thing an admin prices
into a pool — so it migrates in **every** profile, while demo/dev fixtures — three tournaments,
seeded managers, a full recorded result set — live in a second Flyway location,
`db/seed/` (`V3__demo_fixtures.sql`, plus `V6__demo_draft_picks.sql` — a separate file only because
Flyway orders every location by version, and `match_hero_pick` does not exist until V5), that only
`dev` and `test` add to `spring.flyway.locations` (see
Profiles below). A default start or the `prod` profile therefore comes up with every hero and board
and no mock league data at all.

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

Spring Boot is pinned to 4.1.0, which manages Testcontainers 2.0.5 — verified against Docker Engine
29.7.1 (API 1.55). Re-verify against your own Docker Engine before downgrading toward the
3.5.x/Testcontainers 1.21.x line: that line negotiates a Docker API version too old for Engine 29+.

## Profiles

| Profile | Auth | Notes |
|---|---|---|
| `dev` | `DevManagerAuthenticationFilter` resolves `X-Manager-Id` once at the filter level, and *only* when the header is there — no header is an anonymous request, exactly as no bearer token is in `prod`, so a public route costs no manager lookup and anything gated needs the header; `DevManagerProvider` just reads the result back off `SecurityContextHolder` | DEBUG logging, plus `db/seed` added to `spring.flyway.locations` so an admin manager (*NeonStrategist*, in the fixture) and the rest of the demo data actually exist |
| `test` | same dev stub | Testcontainers Postgres via `@ServiceConnection`; same `db/seed` addition, since every integration test asserts against the fixtures |
| `prod` | `SupabaseAuthenticationConverter` verifies the Supabase JWT, resolves `sub` → `manager.auth_user_id`, JIT-provisions, once per request; `SupabaseManagerProvider` just reads the result back off `SecurityContextHolder` | Needs `DB_URL`/`DB_USER`/`DB_PASSWORD`/`SUPABASE_JWKS_URI`, plus optional `FRONTEND_ORIGIN` (only for a frontend calling the API cross-origin instead of through the Worker proxy) |

There are no scheduled tasks and no background workers in any profile, with one narrow exception:
`StandingsSseHub` runs a single-thread keep-alive that pings open standings SSE connections every
20s so idle-timeout proxies/browsers don't silently drop them. It's transport plumbing for the live
standings feed below, not a business-logic worker — it has no DB access and does nothing but write
SSE comment lines to already-open connections.

**Which routes need an identity is decided in exactly one place, for every profile**: the private
`apiAuthorizationRules` function in `SecurityConfig.kt`, passed to `authorizeHttpRequests` by both
chains. It allowlists the read-only GETs that viewing a tournament needs — nobody needs an account
to browse tournaments, hero pools, standings or match history, only to enter and draft — and
everything else under `/api/**` is `authenticated()`, with `anyRequest()` a `denyAll()` backstop. Keep
it in step with `SecurityConfigTest` and `DevSecurityConfigTest`, which assert it from either side.
Don't re-inline it into one chain: the two chains are meant to
differ only in how a credential is *verified* (a Supabase JWT vs. an `X-Manager-Id` header), never in
which routes require one — which is also why neither `SupabaseAuthenticationConverter` nor
`DevManagerAuthenticationFilter` carries any route knowledge of its own. Each resolves an identity
only when the request actually offered a credential; a request without one stays anonymous and lets
the rules above decide, so no public GET pays a JWT verification or a manager lookup.

Both chains also register `RateLimitFilter` (`com.umfl.ratelimit`), an IP-keyed token-bucket
(bucket4j) throttle ahead of every `/api/**` route — `addFilterBefore` puts it ahead of
`BearerTokenAuthenticationFilter` in `SecurityConfig` and ahead of `AuthorizationFilter` in
`DevSecurityConfig`, so a flood doesn't pay JWT verification or the dev manager lookup either. It
keys on `HttpServletRequest.remoteAddr` by default, because a forwarded-for header from a peer that
could itself be the flooder is worthless. `X-Forwarded-For` is read *only* when the peer falls inside
`RateLimitProperties.trustedProxies` — loopback plus the RFC1918 ranges, which is what a
TLS-terminating reverse proxy on the same VPS looks like (note it arrives as the Docker bridge
gateway, e.g. `172.17.0.1`, not `127.0.0.1`, even when the container is published on
`127.0.0.1:8080`). Without that carve-out a proxied deployment puts the entire internet in one
bucket. `RateLimitFilter.clientIp` reads the **last** forwarded entry, not the first: a proxy appends
the address it saw, so the trailing entry is the only one it vouches for, and reading the first would
let a flooder mint a fresh bucket per request with a fake prefix. A backend exposed directly on a
public interface never matches a trusted range and keeps the original behaviour. The tradeoff moves
one hop out rather than disappearing: traffic arriving through the Cloudflare Worker still shares a
bucket per Cloudflare edge IP rather than per visitor. The
per-IP bucket cache is Caffeine-backed (`RateLimitProperties.maxTrackedIps`, LRU-evicted, entries
expire after two quiet refill periods) rather than an unbounded map, since the key space is every IP
that ever touches `/api/`. Tuning (`capacity`, `refillPeriod`, `maxTrackedIps`, `trustedProxies`) lives in
`RateLimitProperties` bound to `rate-limit.api.*` in `application.yml` — see the `umfl.*` invariant
below for why this is the one `@ConfigurationProperties` block in the app despite that rule.

Admin routes (`/api/admin/**`) therefore require `hasRole("ADMIN")` in both profiles — the role comes
from `manager.is_admin`, our own data, resolved once per request by
`SupabaseAuthenticationConverter`/`DevManagerAuthenticationFilter` via the shared, provider-agnostic
`ManagerAuthorities` — never from an identity-provider claim, so swapping auth providers later never
touches the role logic. `DevSecurityConfig` (`!prod`) is also what stops Spring Security's
autoconfiguration from securing everything the moment the oauth2 starter is on the classpath. Don't
delete it.

Those matcher lists are the first layer, not the only one: `MethodSecurityConfig` turns on
`@EnableMethodSecurity`, and every admin controller also carries `@PreAuthorize("hasRole('ADMIN')")`.
The annotation travels with the code, so a future admin endpoint that forgets a URL matcher is still
gated — two independent layers is the point, so keep both in step rather than collapsing one into
the other.

Adding a Supabase project also requires the Discord provider configured in the Supabase dashboard —
the backend never talks to Discord, only to Supabase-signed tokens.

## Backend architecture

Package-by-feature under `com.umfl`, one package per domain concept, plus `api` (controllers +
DTOs), `auth`, `common`, `config` and `ratelimit`.

**The read/write split is the central convention**, and the class names carry it: a plain
`*Repository` is the Spring Data JDBC write/aggregate-loading side, a `*Query`/`*QueryRepository` is
a hand-written `JdbcClient` read projection, and an `*AdminRepository` is a `JdbcClient` *write* for
the composite-keyed link tables Spring Data JDBC can't map. Don't reach for a repository derived-query
method when the screen wants a joined projection, and don't add an ORM.

`TournamentEntry` is the manager-facing aggregate root — it owns `EntrySlot`s via `@MappedCollection`
(list index → `entry_slot.slot_index`) and is saved as one unit. `manager` is written on
JIT-provisioning in `prod`. Everything else is written only through the Admin API, whose own
aggregates (`TournamentMatch`, `ScoringRuleSet`, `Hero`, `GameMap`) are described under Admin API
below; nothing outside that surface writes reference data or results.

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
  reintroduce a tunables block in `application.yml`. The one exception is `rate-limit.api.*`
  (`RateLimitProperties`, see Profiles above): it's operational tuning for how hard a deployed
  instance throttles per client IP, not domain data, so it doesn't belong in the database the way a
  scoring weight or a budget does.
- **A match is a series, and every game in it has a winner.** `tournament_match` is a best-of-N
  between two humans; each `match_game` carries its own map and its own two
  `match_game_participant` rows, so a side can pilot a different hero per game. Exactly one of those
  two rows is flagged `is_winner` — a partial unique index stops two, and
  `MatchResultPolicy.NOT_EXACTLY_ONE_WINNER` stops zero. **There is no draw**, and the loser never
  survives: `MatchResultPolicy.LOSER_HAS_POSITIVE_HEALTH` requires the losing side to finish on 0 or
  less (an overkill hit lands it below zero), and every recorded game in `V3__demo_fixtures.sql`
  respects that. Nothing stores who won the *series*: `MatchListAdmin` counts games won client-side,
  like every other derived number here.
- **The draft is recorded in full, as picks *and* bans.** `hero_ban` holds the heroes struck out of a
  series; `match_hero_pick (match_id, side, hero_id)` holds the heroes each side took. Both are per
  series, never per game. A recorded draft is *complete* — `MatchResultPolicy.PLAYED_HERO_NOT_DRAFTED`
  rejects a game whose hero is missing from that side's picks — which is what lets `APPEARANCE` be
  "was drafted and not banned" rather than "played". `BANNED_HERO_DRAFTED` keeps the two halves
  disjoint. There is deliberately no `unique (match_id, hero_id)` on the picks: games are
  independent, so a hero may legitimately go to one side in game 1 and the other in game 2, and
  `MatchResult.draftedHeroIds` de-duplicates instead.
- **No `player` entity.** Every point is scored per *hero*: no metric extractor, no coefficient and
  no standings query reads the human who piloted it. So the competitor is
  `match_participant.player_label` — one row per side for the whole series, nullable free text with
  no table, no FK, no repository and deliberately no admin API. An admin records a new competitor by typing their name. It is display
  text for the ticker and the admin match list, nothing more, and `MatchResultPolicy` never validates
  it (a blank label normalises to null in `AdminMatchService`). Promote it to a real table only if
  something starts scoring or ranking the humans — until then, a `player` table only buys you CRUD
  you have to build and a foreign key that can 500.

Domain rules live in pure `object`s with no Spring or persistence dependency — `RosterPolicy`,
`MatchMetrics`, `ScoringEngine`. New rules belong there, tested directly, not inside a `@Service`.

`RosterPolicy.validateDraft` deliberately permits over-budget selections (the builder is a
scratchpad, the meter just runs past 100%); `validateLock` adds the budget and roster-size checks.
Both return *all* violations at once so the UI can highlight every problem in one pass. The rule
codes are the `RosterRule` enum, documented constant by constant.

`MatchMetrics` is a registry keyed by the free-form `scoring_coefficient.metric` string. It
implements `APPEARANCE`, `SELF_BAN`, `OPPONENT_BAN`, `WIN`, `LOSS`, `HEALTH_REMAINING`,
`HEALTH_DIFFERENTIAL`, `SHUTOUT`, and **silently ignores everything else** — unknown keys score
zero, are dropped from the leaderboard's columns and throw nothing. The seed's `CROWD_FAVOURITE` is
the deliberate proof of that; leave it unimplemented. There is deliberately no `DRAW`: every game
has exactly one winner (see the invariant above), so `WIN` and `LOSS` are exhaustive within a game
and a `DRAW` column would price something that cannot be recorded. Extractors take a `MetricContext`
(the hero's role in one match — `Played`, which is scoped to *one game* of it, or the per-series
`Drafted`/`Banned`), not a bare participant row, because
`HEALTH_DIFFERENTIAL` needs the opponent and `APPEARANCE`/`SELF_BAN`/`OPPONENT_BAN` have no
participant row at all — they price the draft, reading `hero_ban.ban_type` off the match or the role
itself, so a hero banned `PRE_BAN` (struck before sides are known) scores neither ban metric.
`HEALTH_DIFFERENTIAL` is also win-gated: a hero that did not win
the game scores 0.0 rather than a negative differential, since there is no losing side of that
metric to price. `WIN`/`LOSS` are scored per game, not per series, so a hero that takes game 1 and
drops game 2 of a Bo3 collects one of each.

`MatchResult.heroContexts()` is where per-game and per-series part ways, and the split is the whole
reason `APPEARANCE` is not multiplied by series length: a hero that played yields one `Played`
context *per game* plus exactly one `Drafted` context, a hero drafted and never fielded yields only
the `Drafted` one, and a banned hero yields only `Banned`. `StandingsService.ticker` has to bridge
that, since its rows are games: it banks the `Drafted` context against the hero's **first** game so
the ticker's per-game points still sum to what the board gained, and names the never-fielded picks
separately as `draftedUnplayedHeroNames`.

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
through `GlobalExceptionHandler` as RFC 7807 problem details: the domain exceptions in
`common/DomainExceptions.kt` each map to a status there, and the three `*RuleException` types all
render 422 with a `violations` array. Throw one of those rather than a bare `ResponseStatusException`.

`GlobalExceptionHandler` **extends `ResponseEntityExceptionHandler`, and must keep doing so**:
`ExceptionHandlerExceptionResolver` runs ahead of `DefaultHandlerExceptionResolver`, so the
`@ExceptionHandler(Exception)` catch-all intercepts Spring MVC's own exceptions before the resolver
that knows their real status — without the base class an unparseable path variable, a malformed body,
a wrong HTTP method and a `@PreAuthorize` denial all answered 500. That inheritance is also a
constraint: the base already maps every type listed on its `handleException`, and mapping one twice
is an ambiguous-handler error at *context startup*, so customise anything in that set by overriding
the matching `protected` hook (as `handleMethodArgumentNotValid` does to keep the `fields` property),
never with a second `@ExceptionHandler`. `GlobalExceptionHandlerMvcTest` pins the statuses through a
real dispatch — a direct unit call cannot see resolver ordering, which is the whole bug.

`HeroSort` holds ORDER BY fragments as an enum whitelist because sort keys can't be parameterised —
keep new sorts inside that enum.

Migrations are `backend/src/main/resources/db/migration/V*__*.sql`, forward-only, squashed to a
single `V1__core_schema.sql` baseline (see Commands above for why, and for the seed's separate Flyway
location). Tables the app loads or writes as aggregates carry a surrogate `bigserial` id next to a
unique natural key because Spring Data JDBC can't map composite primary keys; the pure link tables
(`tournament_hero`, `tournament_map`, `entry_slot`, `hero_ban`, `match_hero_pick`,
`match_game_participant`) keep natural composite keys. The
integration tests assert on the seed's numbers exactly — changing a seeded price or result means
updating `V3__demo_fixtures.sql` and the tests together. New *league* data past that fixed baseline
goes through the Admin API (see below), not another migration. The one thing that legitimately
arrives as a migration is reference data: a hero or a board Restoration Games releases later belongs
in a forward migration alongside `V2__reference_data.sql` (or through `/api/admin/heroes` /
`/api/admin/maps`, which write the same tables), since a `prod` database has to have it without
anyone hand-entering 74 heroes.

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
`/standings/stream` SSE connection (`src/api/sseClient.ts`) and calls `refresh()` every time the
backend signals that a match was written. `refresh()` always refetches the full ticker head from
`sinceMatchId=0` rather than incrementally — a correction reuses an existing match id and a deletion
removes one, so an incremental "since" fetch could miss either. There's still no client-side polling
timer; the server-pushed event is what triggers each `refresh()`. The stream is closed and reopened
on tournament switch and closed on unmount.

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
from the active rule set and isn't knowable in advance. Rank and Manager stay pinned while it
scrolls, which costs a few non-obvious rules in `main.css` — they're commented where they live, next
to `.cell-pinned`.

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
rejected with a `ConflictException` when the tournament has a recorded game on it, since
`match_game` carries a composite FK onto that row. That FK is `DEFERRABLE INITIALLY DEFERRED` so a
tournament delete (which cascades to `tournament_map` and, one level deeper, to `match_game`) is not
tripped by cascade ordering — which is why `AdminMapService.removeFromPool` calls
`MapPoolAdminRepository.checkMapInPoolNow()` (`set constraints … immediate`) after its DELETE: the
violation has to surface inside the method that can still name the map, not at commit.

The reference and result tables have a Spring Data JDBC write side: `Hero`, `GameMap`,
`TournamentMatch` (owning `participants` as a `List` keyed by `side`, plus `games`, `bans` and
`picks` as `Set`-mapped children — no `keyColumn` there,
since none carries a list-position column; each `MatchGame` in turn owns its own participants)
and `ScoringRuleSet` (owning `coefficients`). `picks` hangs off the match root rather than off
`MatchParticipant`, where it would read more naturally: `match_participant` is composite-keyed and
Spring Data JDBC cannot map a child of an entity keyed that way. The *API* still hangs the draft off
the side that owns it — `MatchParticipantRequest.draftedHeroIds` in, `MatchParticipantResult.draftedHeroes`
out, with `side` the list position as it already is for `player_label` — and `AdminMatchService.toPicks`
does the transposition, which is also why an out-of-range side is unrepresentable and no rule polices
one. `tournament_hero` and `tournament_map` stay
composite-keyed link tables with no Kotlin entity — `HeroPoolAdminRepository`/`MapPoolAdminRepository`
write them via `JdbcClient`, the same read/write split the rest of the app already uses.

`MatchResultPolicy` (pure, mirrors `RosterPolicy`) validates a match submission before save, raising
`MatchRuleException` (422, same shape as `RosterRuleException`, kept as a separate type rather than
merged into it). The checks are the `MatchRule` enum, one KDoc line each — read them there. Two
enforce the no-draw invariant above and are the ones to know before touching the policy:
`NOT_EXACTLY_ONE_WINNER` treats zero winners as being as wrong as two, and
`LOSER_HAS_POSITIVE_HEALTH` rejects a loser who survived. Three more police the draft:
`PLAYED_HERO_NOT_DRAFTED` is what makes a recorded draft complete (and so what makes `APPEARANCE`
measurable), `BANNED_HERO_DRAFTED` keeps picks and bans disjoint, and `DUPLICATE_PICK` mirrors
`DUPLICATE_BAN`. Activating a scoring rule set deactivates any active sibling in the same transaction, since only one may be active per tournament. An unknown
scoring metric (e.g. the seed's `CROWD_FAVOURITE`) is surfaced as a non-blocking warning on the
response, never rejected.

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
price is the whole point of the warning. Keep the array in step when adding an extractor — and note
`DRAW` is *not* in it, deliberately, so pricing one is flagged like any other metric this build
cannot measure.

`MatchResultWizard` records a whole series: one map and two heroes per game, "+ Add Game" for a
best-of-N, and the two player names once for the match. Each game's winner is a **radio**, not a
checkbox — exactly one side wins, and there is no "neither" to express — with a client-side check
before save so an untouched winner reads as a prompt instead of a 422. Each side also carries its
**draft**, and the form only asks for the half it cannot derive: the heroes that side fields in the
games are folded into `draftedHeroIds` at save time by `draftFor`, so the admin types only the picks
that never played, and `PLAYED_HERO_NOT_DRAFTED` can never fire on a submission this UI built.
`MatchListAdmin` renders the games grouped under their maps, derives the games-won tally per side,
and names each side's drafted-but-unfielded picks — the heroes that scored an appearance without
appearing in any game row.

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
`frontend/src/worker.ts` is what routes the deployed frontend to the backend. The frontend deploys as
a **Cloudflare Worker with static assets** (`frontend/wrangler.toml`), not a Pages project, so
`public/_redirects` would never be read — the Worker proxies `/api/*` to `env.BACKEND_HOST` by hand
and serves everything else from the `ASSETS` binding, falling back to `index.html` so Vue Router's
history mode survives a deep link. The effect is the same same-origin proxy: the frontend's relative
`/api/...` calls (`client.ts`, `sseClient.ts`) reach the VPS with no cross-origin request involved,
which is why `FRONTEND_ORIGIN` stays unset in `prod`. `BACKEND_HOST` is a plain `[vars]` entry in
`wrangler.toml` rather than a secret (it's just a base URL) — point it at the real backend hostname,
and keep the `server.proxy` target in `frontend/vite.config.ts` in step so dev and prod hit the same
API.
