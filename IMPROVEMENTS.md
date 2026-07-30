# Improvement Backlog — um-fantasy-league

Audit date: **2026-08-04**, against `master` @ `5449e02` plus the three uncommitted
working-tree changes (`application.yml`, `deploy/docker-compose.prod.yml`,
`AdminDashboardView.vue`).

Each item is self-contained: file/line evidence, why it matters, and what "done"
looks like. Pick one up without reading the others.

## Framing

The domain core is in good shape and should not be the target of this work. The
read/write split, the pure-`object` policies (`RosterPolicy`, `MatchResultPolicy`,
`MatchMetrics`, `ScoringEngine`), the derived-at-read-time scoring, and the
two-layer admin authorization are all consistent, well-tested, and documented with
their rationale. The backend's 23 test classes cover it properly.

Almost every finding below is on the **newer perimeter**: the admin frontend, the
admin API's read surface, rate-limit lifecycle, and deployment plumbing. That is
where the codebase's own conventions are not being followed and where CI proves
the least.

Severity: **P1** = broken or actively harmful today · **P2** = blocks real admin
work or will bite under load · **P3** = cleanup.

---

# P1 — Broken today

## F1. 99 references to CSS variables that do not exist

**Files:** all 9 admin components + `frontend/src/views/AdminDashboardView.vue`
**Severity:** P1 (visual) · **Effort:** M · **Owner hint:** frontend

`frontend/src/assets/main.css` `@theme` defines the token set. These six names are
used but **never defined anywhere**:

| Variable | Uses | Intended token in `@theme` |
|---|---|---|
| `--color-accent-cyan` | 33 | `--color-cyan` |
| `--color-surface-raised` | 22 | `--color-surface-mid` / `-high` |
| `--color-surface-base` | 20 | `--color-surface` / `--color-surface-low` |
| `--color-accent-magenta` | 18 | `--color-magenta` |
| `--color-accent-red` | 4 | `--color-danger` |
| `--color-accent-lime` | 2 | `--color-lime` |

An undefined `var()` with no fallback makes the whole declaration invalid at
computed-value time — the border, background or colour silently falls back to
inherited/initial. Every admin panel, form field, focus ring and status colour is
affected.

Same bug in utility-class form: `AdminDashboardView.vue:103` and `:127` use
`text-accent-cyan`. Tailwind v4 generates utilities from `@theme` names, so the
generated class is `text-cyan`; `text-accent-cyan` emits nothing.

Note the uncommitted `AdminDashboardView.vue` diff **adds** two more
(`--color-surface-base`, `--color-accent-cyan` in the new `.form-input` rules).

**Reproduce / verify:**

```bash
cd frontend/src
grep -ohE "var\(--[a-z0-9-]+" components/*.vue views/*.vue layouts/*.vue | sort -u > /tmp/used.txt
grep -ohE "^\s*--[a-z0-9-]+" assets/main.css | tr -d ' ' | sort -u > /tmp/defined.txt
while read -r v; do n="${v#var(}"; grep -qx -- "$n" /tmp/defined.txt || echo "$n"; done < /tmp/used.txt
```

**Done when:** that command prints nothing, and `grep -rn "accent-cyan\|accent-magenta\|accent-lime\|accent-red\|surface-base\|surface-raised" frontend/src` is empty.

**Do not** fix this by adding aliases to `@theme`. AGENTS.md names the token
vocabulary deliberately; adding a second set of names for the same colours is the
problem, not the fix. See also **F2**, which is the same task at a larger radius.

---

## F3. Recording a match requires typing raw numeric database IDs

**File:** `frontend/src/components/MatchResultWizard.vue`
**Severity:** P1 (usability/correctness) · **Effort:** M · **Owner hint:** frontend

`MatchResultWizard.vue:187` — `placeholder="Enter map ID"`. Hero selection is the
same: `blankForm()` seeds `heroId: 0`, and validation is
`'All participants must have a hero ID'` (line ~100). An admin recording a real
result has to know the numeric primary key of one map and two heroes.

Both lookups already exist in `src/api/client.ts` and are unused here:

- `api.admin.listMapPool(tournamentId)` → the tournament's legal boards
- `api.admin.listHeroes()` → all hero identities

A wrong id comes back as `MAP_NOT_IN_POOL` or `UNKNOWN_HERO` — correct 422s from
`MatchResultPolicy`, but they never tell the admin what the right id *was*.

**Done when:** map is a `<select>` populated from `listMapPool`, participants and
bans pick heroes from a `<select>` (ideally the tournament's priced pool via
`api.heroes(tournamentId)`, which is what the match actually scores against), and
the numeric-id inputs are gone. Cover the form → `RecordMatchRequest` mapping with
a vitest spec (see **F6**).

---

## D1. A regenerating tunnel hostname is hardcoded in committed source

**File:** `frontend/src/worker.ts:9`
**Severity:** P1 · **Effort:** S · **Owner hint:** infra

```ts
const BACKEND_HOST = 'https://unity-consider-weather-aware.trycloudflare.com'
```

`trycloudflare.com` quick tunnels get a fresh random hostname on every restart.
The moment that tunnel drops, every `/api/*` call from the deployed frontend
404s — and fixing it requires a code change plus a redeploy.

**Done when:** the host comes from a `wrangler.toml` `[vars]` binding (or a
secret), read off `env` inside `fetch`, with the current value set in the
Cloudflare dashboard rather than in git. The `Env` interface at the top of the
file already exists to extend.

---

# P2 — Blocks real work, or will bite under load

## B1. `RateLimitFilter`'s bucket map grows without bound

**File:** `backend/src/main/kotlin/com/umfl/ratelimit/RateLimitFilter.kt:38,46`
**Severity:** P2 · **Effort:** S–M · **Owner hint:** backend

```kotlin
private val buckets = ConcurrentHashMap<String, Bucket>()
...
val bucket = buckets.computeIfAbsent(request.remoteAddr) { newBucket() }
```

Nothing ever removes an entry. Every distinct source IP that touches `/api/`
allocates a `Bucket` that lives for the JVM's lifetime.

This matters more than usual here because of a design decision the class doc
already states: the VPS port is reachable directly with no reverse proxy in
front, and the filter deliberately keys on `remoteAddr` rather than a spoofable
`X-Forwarded-For`. So the map's key space is "every IP on the internet that
scans port 8080", and the prod container runs under `mem_limit: 768m`
(`deploy/docker-compose.prod.yml`).

**Done when:** buckets are held in a bounded, expiring cache — e.g. Caffeine with
`expireAfterAccess(refillPeriod * 2)` and a `maximumSize`, or bucket4j's own
`ProxyManager`. A periodic sweep is acceptable but prefer not to add a second
background thread; `StandingsSseHub`'s keep-alive is the one documented exception
to "no scheduled tasks" and that exemption should stay narrow. Extend
`RateLimitFilterTest` with an eviction case.

---

## B4. The admin API has no read surface for scoring, and no way to undo a pool edit

**Files:** `api/AdminScoringController.kt`, `hero/HeroPoolAdminRepository.kt`,
`map/MapPoolAdminRepository.kt`
**Severity:** P2 · **Effort:** M · **Owner hint:** backend (unblocks **F4**)

Three gaps, all in the same shape — the write side exists, the read/undo side
doesn't:

1. **No `GET .../scoring-rule-sets`.** `AdminScoringController` exposes only
   `POST`, `PUT /{id}` and `POST /{id}/activate`. There is no way for an admin to
   see which rule sets exist or which one is active — which is why
   `ScoringRuleSetWizard` is create-only (**F4**) and why
   `api.admin.updateScoringRuleSet` / `activateScoringRuleSet` in `client.ts` are
   dead code.
2. **No `GET /api/admin/tournaments/{id}/heroes`.** `HeroPoolWizard.vue:54` reads
   the *public* `api.heroes(tournamentId)` instead. Works, but the admin pool view
   and the player-facing pool view are now the same endpoint with the same
   filters, so they can't diverge later without a rewrite. Compare
   `AdminMapController.listPool`, which does have an admin-scoped listing.
3. **No remove-from-pool.** `HeroPoolAdminRepository` has only `upsertCost`;
   `MapPoolAdminRepository` has only `addToPool`. A hero priced into the wrong
   tournament, or a map added by mistake, can never be taken out through the API.

**Constraint for whoever implements removal** — the schema will fight you, and
that is intended:

- `entry_slot.hero_id references heroes(id)` with **no cascade**
  (`V1__core_schema.sql:146`), deliberately, so a rostered hero can't be quietly
  removed. Removing a hero from `tournament_hero` doesn't hit that FK, but it
  *does* re-price every unlocked roster holding it to 0 via
  `HeroQueryRepository.findRosterHeroes` — decide and document whether that's
  allowed.
- `tournament_match` carries a composite FK onto `tournament_map`
  (`V1__core_schema.sql:227`), so removing a map from a pool with recorded
  matches must be rejected as an explicit `ConflictException`, not left to
  surface as a raw FK error.

**Done when:** the three endpoints exist with `@PreAuthorize("hasRole('ADMIN')")`
*and* matching `/api/admin/**` matchers in both `SecurityConfig` and
`DevSecurityConfig` (AGENTS.md: keep both layers in step), `client.ts` +
`types.ts` are updated first as the contract anchor, and each has an integration
test alongside the existing `Admin*ServiceIntegrationTest` classes.

---

## F4. `ScoringRuleSetWizard` is create-only and silently swallows metric warnings

**File:** `frontend/src/components/ScoringRuleSetWizard.vue`
**Severity:** P2 · **Effort:** M · **Owner hint:** frontend · **Depends on:** B4

The component calls exactly one endpoint (`createScoringRuleSet`, line 99). There
is no list, no edit, no activate. Consequence: **an admin cannot change which
scoring rule set is active**, so the leaderboard's columns and weights are
effectively frozen at whatever the seed activated.

Second, sharper problem: the response's `warnings` field is discarded. Per
AGENTS.md, an unknown metric is deliberately a *non-blocking warning* rather than
a rejection (`ScoringRuleSetDto.warnings`, fed by `MatchMetrics.unknown`). Nothing
renders it. So an admin who types `HEALH_REMAINING` gets a clean `201`, a column
that scores zero forever, and no signal at all. The `commonMetrics` array at line
23 duplicates `MatchMetrics`' registry as a hint list but doesn't validate against
it.

**Done when:** the wizard lists existing rule sets with their active flag, can
edit and activate one, and renders `warnings` prominently after a save. A vitest
spec should cover the coefficient add/remove/`sortOrder`-renumber logic (lines
53–68), which is fiddly and currently untested.

---

## B2. A duplicated metric degrades to the generic "should never fire" 409

**File:** `backend/src/main/kotlin/com/umfl/scoring/AdminScoringService.kt:85-88`
**Severity:** P2 · **Effort:** S · **Owner hint:** backend

```kotlin
private fun toCoefficients(coefficients: List<ScoringCoefficientInput>): Set<ScoringCoefficient> =
    coefficients.map { ScoringCoefficient(metric = MatchMetrics.normalise(it.metric), ...) }.toSet()
```

`.toSet()` dedupes on full data-class equality, not on `metric`. A submission of
`[{WIN, 1.0}, {WIN, 2.0}]` yields two set members, and
`scoring_coefficient` has `unique (rule_set_id, metric)`
(`V1__core_schema.sql:203`). The insert throws
`DataIntegrityViolationException`, caught by the backstop in
`GlobalExceptionHandler.kt:65` — whose own doc comment says *"every known case is
caught earlier by a domain policy... this should never fire in practice."* The
admin gets `409 "The request conflicts with existing data."`

Same path for a metric that survives `normalise` but fails the schema's
`^[A-Z][A-Z0-9_]*$` CHECK — e.g. `win-rate` → `WIN-RATE`, or `1st` → `1ST`.

Every other admin write in this codebase pre-validates with a pure policy object
and returns a named rule code. This one doesn't.

**Done when:** a `ScoringRuleSetPolicy` (pure `object`, mirroring
`MatchResultPolicy`) rejects duplicate metrics and malformed metric names with
named rules, raising a 422 with a `violations` array. Test it directly like
`MatchResultPolicyTest` — no Docker needed, which makes it one of the cheap
high-value gates AGENTS.md points at.

---

## F6. The admin dashboard has zero test coverage

**Severity:** P2 · **Effort:** M · **Owner hint:** frontend

7 spec files exist, covering stores, router, domain policy and the shell. **None**
of the 9 admin components (~2,900 lines, the largest single body of code in the
frontend) has one.

`frontend-ci.yml` runs `npm run type-check` and `npm test` as the merge gate. Both
pass on the current tree — **F1** and **F3** are invisible to it. That gate is
currently proving nothing about the half of the frontend most likely to change.

**Highest-value targets, in order:**

1. `MatchResultWizard` — form ⇄ `RecordMatchRequest` mapping, edit-mode round-trip
   through `getMatch`, `setWinner` mutual exclusion.
2. `ScoringRuleSetWizard` — coefficient add/remove and `sortOrder` renumbering.
3. `MatchListAdmin` — the round filter (see **F5**, which it would have caught).

---

## D2. The documented deployment mechanism does not exist

**Severity:** P2 · **Effort:** S (docs) + M (CI) · **Owner hint:** infra

AGENTS.md states that Cloudflare **Pages** builds `frontend/`, and that
`frontend/public/_redirects` proxies `/api/*` to the VPS.

Actual state of the repo:

- `frontend/public/` is **empty** — there is no `_redirects` file.
- `frontend/wrangler.toml` + `frontend/src/worker.ts` describe a Cloudflare
  **Workers** deploy with an `ASSETS` binding and a hand-rolled `/api/*` proxy.
- No workflow runs `wrangler deploy`. Frontend deployment is entirely manual and
  undocumented.

Two backend source comments cite the phantom file as load-bearing rationale and
will mislead the next reader: `config/CorsConfig.kt:33` and
`ratelimit/RateLimitFilter.kt:29`.

**Done when:** AGENTS.md's CI/CD section describes the Workers deploy, those two
Kotlin comments are corrected, and either a `frontend-deploy.yml` runs
`wrangler deploy` on `master` or the manual step is written down. Fold **D1** in
while you're here — same file, same deploy story.

---

# P3 — Cleanup

## B3. Activating a rule set rewrites every coefficient row

`scoring/AdminScoringService.kt:69-78`. `ScoringRuleSet` is an aggregate root
owning `coefficients` via `@MappedCollection`, so each
`ruleSetRepository.save(it.copy(isActive = ...))` deletes and reinserts all child
rows. Merely toggling a boolean churns coefficient ids and burns sequence values
for *both* the outgoing and incoming rule set. A targeted
`JdbcClient` update of `is_active` follows the same read/write-split precedent as
`HeroPoolAdminRepository`. (The two-statement ordering against the partial unique
index is correct as written — keep it.)

## B5. Tournament capacity check is check-then-insert

`tournament/TournamentService.kt:61`. `unique (tournament_id, manager_id)` covers
the double-registration race at the DB level, but nothing enforces `capacity` —
two concurrent registrations for the last seat both commit. Either take a row
lock on the tournament, or decide it's acceptable at club scale and say so in the
method doc. Currently it's neither.

## B7. `/api/tournaments` loads full entry aggregates to read one enum

`api/TournamentController.kt:96-99` → `entryRepository.findByManagerId(id)`. That
returns `TournamentEntry` aggregates, so Spring Data JDBC issues a child query per
entry to populate `entry_slot` — all to read `status`. A `JdbcClient` projection
(`select tournament_id, status from tournament_entry where manager_id = :id`) is
exactly the read/write split the rest of the codebase uses, and
`idx_tournament_entry_manager` already backs it. Fires on every lobby load.

## B6. A single match write can fan out to ~1000 requests

`StandingsSseHub` allows `MAX_TOTAL_SUBSCRIBERS = 500`. Each `StandingsUpdateEvent`
pushes to all of them, and every client then calls `/standings` **and**
`/matches`. `StandingsService.board` reloads every match in the tournament (3
queries via `MatchResultQuery.assemble`) plus every roster, then folds in Kotlin.

Fine at the scale this is built for, and the "never materialise points" invariant
is right. But if subscriber counts grow, memoise the board on
`(tournamentId, max(matchId))` — a key that's already monotonic and already
central to the ticker's design — rather than reintroducing a stored total.

## B8. Small backend items

- `api/AdminHeroController.kt:15` — imports `RequestMapping`, never uses it.
- `api/AdminMatchController.kt:64` — fully-qualified
  `com.umfl.common.NotFoundException` inline; every sibling imports it.
- `match/MatchResultQuery.kt:36-40` — `findByTournament` concatenates an optional
  `roundClause` and conditionally rebinds `spec`. A single
  `(:round is null or m.round = :round)` predicate with an always-bound param
  removes the branch and the mutable `var`.
- `match/MatchResultPolicy.kt` — doesn't reject a hero submitted as both a
  participant and a ban in the same match. `MatchResult.heroContexts()` resolves
  it defensively in favour of "played", with a comment saying this is for *"a bad
  record"*. Validating at write time would keep that read-side branch a genuine
  backstop rather than a supported input.
- No `ktlint` / `detekt` / `spotless` in the Gradle build. The Kotlin is unusually
  consistent, so this is about keeping it that way — and it would have caught the
  two items above for free.

## F2. The admin UI reimplements the design system in scoped CSS

~2,400 lines of `<style scoped>` across the 9 admin components hand-roll form
fields, buttons, panels and tables. `main.css @layer components` already ships
`.panel`, `.label-caps`, `.stat-value`, `.headline`, `.btn-primary`, `.btn-ghost`,
and `@theme` ships the token vocabulary AGENTS.md tells you to use
(`bg-surface-low`, `text-ink-dim`, `border-edge`, `font-display`).

This divergence is the *root cause* of **F1** — hand-written CSS invented plausible
variable names because it wasn't reaching for the utilities. Compare
`RosterBuilderView.vue:88`, which uses the tokens correctly and needs no scoped
CSS at all.

Fixing **F1** by consolidating on the shared components/utilities deletes most of
those 2,400 lines rather than repairing them. Treat F1 and F2 as one task if
taking either.

## F5. `MatchListAdmin` leftovers and a filter that half-works

`frontend/src/components/MatchListAdmin.vue`:

- **:53-54** — `// This will be implemented to open the match wizard in edit mode`
  plus `console.log('Edit match:', matchId)`. Both stale; the `emit('edit', ...)`
  on the next line already does the job.
- **Double filtering.** `loadMatches` passes `selectedRound` to
  `api.admin.listMatches` (server-side), *and* `filteredMatches` re-applies it
  client-side. But nothing watches `selectedRound`, so `loadMatches` only reruns
  after a delete — the server-side param is effectively dead, and the two filters
  can disagree. Pick one.
- **:79** — `loadMatches()` runs once at setup; `props.tournamentId` isn't
  watched. Relies on the parent remounting the component.
- **:60** — native `confirm()` for the destructive delete, where
  `TournamentManagementWizard` uses an in-app confirmation step for the same kind
  of action.

## F7. Playwright e2e runs in neither workflow

`frontend/e2e/` needs a live backend and Postgres, so it's excluded — already noted
as known in AGENTS.md. Worth flagging that `responsive.spec.ts` guards the
specific failure mode AGENTS.md calls out (nothing sets `overflow-x` on `body`, so
one over-wide element slides the whole page), and the admin components added since
have never been checked against it. A compose-backed CI job would close the
largest remaining coverage gap after **F6**.

## D3. The prod compose file hardcodes the image owner

`deploy/docker-compose.prod.yml:2` pins `ghcr.io/therealtod/umfl-backend:latest`
while `backend-deploy.yml` pushes to
`ghcr.io/${{ github.repository_owner }}/umfl-backend`. Correct while the owner
matches; a fork or org rename silently deploys someone else's image.
`${IMAGE:-ghcr.io/therealtod/umfl-backend}:latest`, sourced from the box's
hand-managed `.env`, makes the coupling explicit.

---

# Note on the uncommitted working tree

- `application.yml` + `deploy/docker-compose.prod.yml` — graceful shutdown
  (`server.shutdown: graceful`, 30s phase timeout) inside a 35s
  `stop_grace_period`. Coherent and correctly ordered; the 5s margin is right.
  Ready to commit.
- `AdminDashboardView.vue` — replaces the "Use Demo Tournament (ID: 1)"
  placeholder with a real tournament `<select>`. Right change; the store is
  populated by `App.vue`'s `onMounted`, so the dropdown does fill. But the new
  CSS introduces two more undefined variables — fold it into **F1**.

# Suggested order

1. **F1 + F2** together — one pass, deletes code, unblocks confident UI work.
2. **B1** — smallest P1-adjacent fix with real production consequences.
3. **D1 + D2** — same deploy story; makes the frontend deployable without a code edit.
4. **B4 → F4** — backend read surface, then the wizard that needs it.
5. **F3 + F6** — rebuild the match wizard's inputs and land its first specs together.
6. **B2**, then the P3 list as capacity allows.
