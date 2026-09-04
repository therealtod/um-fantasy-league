# AGENTS.md

Guidance for coding agents working in this repository. `CLAUDE.md` just imports this file.

## What this is

A fantasy layer over **real** Unmatched tournaments: an admin records match results, managers draft
heroes under a per-tournament budget, and points are derived from the recorded results by a scoring
table that lives in the database. Nothing is simulated: match results, tournaments, heroes, maps and
scoring rules are real facts an admin enters through the Admin API (`/api/admin/...`), not computed
or randomly generated. A Cargo workspace (`backend-rs/`, two crates — `umfl-domain` for pure domain
logic, `umfl-server` for the axum HTTP server) + a separate npm frontend that is not part of the
Cargo workspace.

`README.md` carries the domain rationale (why `sqlx` over an ORM, why cost is per tournament, why
scoring is data, seed-data tuning). Read it before making design-level changes.

## Commands

```bash
# Database + migrations (Postgres 17 on host port 5433)
docker compose up -d db flyway   # flyway is one-shot: applies db/migration + db/seed, then exits

# Backend — cargo run against that database
cd backend-rs && SPRING_PROFILES_ACTIVE=dev cargo run -p umfl-server

# Frontend (separate shell) — Vite proxies /api to localhost:8080
cd frontend && npm install && npm run dev
```

Migrations are periodically squashed back into a single `V1__core_schema.sql` baseline (schema +
admin authorization, no data — and, as of the most recent squash, every table renamed to a plural
noun: `hero` → `heroes`, `tournament_hero` → `tournament_heroes`, `tournament_match` →
`tournament_matches`, and so on throughout), so an older local database fails Flyway checksum
validation. There is no production data: `docker compose down -v && docker compose up -d db flyway`.
`db/` lives at the **repository root**, not under `backend-rs/`'s own tree, because it is Flyway's
directory rather than the Rust crate's: `db/Dockerfile` and the `flyway` compose services apply it
independently of any backend build, so the SQL is shared infrastructure rather than copied into a
build's resources. Flyway is no longer embedded in the server process at all (see
Profiles below): `db/Dockerfile` bakes `db/migration` and `db/seed` into the stock Flyway CLI image,
and *which* locations a given run applies is decided by that invocation's `FLYWAY_LOCATIONS`, not by
anything the backend process reads — `docker-compose.yml`'s `flyway` service passes both locations,
`deploy/docker-compose.prod.yml`'s passes `db/migration` only. Data past the schema baseline splits by
*kind*, not by environment: `db/migration/V2__reference_data.sql` carries the canonical hero and board
catalogue — facts about Unmatched, not about a league, and the thing an admin prices into a pool — so
it is applied by **every** Flyway invocation, while demo/dev fixtures — three tournaments, seeded
managers, a full recorded result set, draft picks and ban sides all included from the start — live in
a second Flyway location, `db/seed/V3__demo_fixtures.sql`, that only the dev-shaped invocation adds.
There is no longer a `V6__demo_draft_picks.sql` or `V8__demo_ban_sides.sql` — those existed only
because the columns they filled postdated the seed's own migration; a squash folded them back into
`V3` now that everything they depended on is already in `V1`. A `migration`-only Flyway run (what
`deploy/docker-compose.prod.yml` does) therefore leaves every hero and board in place and no mock
league data at all.

The admin match import needs the scraper sidecar. It is a plain Node process — Docker is only how it
reaches the VPS — so a dev session runs it directly, and the backend's default `scraper_base_url`
finds it with no configuration:

```bash
cd tools/tabletopleague-scraper
npm install && npx playwright install chromium   # once
npm run serve                                    # http://127.0.0.1:3000
```

It is optional: the backend starts and every other feature works without it, and only the import
endpoint answers 503.

Tests:

```bash
# Pure domain logic, no Docker needed — the highest-value gate in the repo
cargo test -p umfl-domain

# Full workspace, including sqlx-checked queries and integration tests (needs a running Docker
# daemon — Testcontainers spins up Postgres, clones a migrated template database per test)
cd backend-rs && cargo test --workspace

# Single test
cargo test -p umfl-domain roster_policy::tests::some_test_name

cd frontend && npm test              # vitest run, src/**/*.spec.ts
cd frontend && npm run type-check    # vue-tsc --noEmit
cd frontend && npx vitest run src/domain/rosterPolicy.spec.ts   # single file
```

`backend-rs/crates/umfl-server/tests/it/` is one binary (`tests/it/main.rs` with `mod` declarations),
one Postgres container, one migrated `umfl_template` database — each test does
`CREATE DATABASE umfl_test_<n> TEMPLATE umfl_template` (Postgres file-copies it) rather than sharing
one database and rolling back a transaction, because a service under test calls `state.pool.begin()`
and gets a *different connection* than the test's own, which would see none of a rolled-back
transaction's uncommitted fixtures. That has three consequences worth knowing: an after-commit-shaped
listener actually fires during the suite (there is no rolled-back-transaction caveat to work around,
so `StandingsSseHub`'s keep-alive push is exercised end to end rather than only unit-tested); nothing
needs a per-test cache-invalidation hook, since `MatchResultCache` lives on `AppState` and every test
constructs its own; and the suite runs in parallel by default, since every test owns a database cloned
from the template with nothing to contend over — mark a test `#[serial]` only if it genuinely needs
the whole binary to itself (there is currently exactly one `SELECT … FOR UPDATE` path, the tournament
capacity check, and it's already isolated per-database).

Formatting:

```bash
cd backend-rs
cargo fmt --all --check     # merge gate, alongside `cargo test --workspace`
cargo fmt --all             # auto-fix
cargo clippy --workspace --all-targets -- -D warnings
```

`sort_unstable_*` is **banned** by `clippy.toml`'s `disallowed-methods` (see the `stable_sort_primitive`
allowance in the workspace `Cargo.toml` for why the built-in clippy lint is off instead): the
standings ranking (standard competition ranking: 1, 2, 2, 4) and the ticker's winner-first participant
sort both depend on everything they did *not* compare keeping the order the database returned it in.
Use `sort_by`/`sort_by_key`.

`cargo sqlx prepare --workspace -- --all-targets` (with `DATABASE_URL` pointing at a migrated
database) has to be re-run and its `.sqlx/` output committed before any commit that changed a
`sqlx::query!`/`query_as!` call — forgetting it still compiles locally (there's a live database to
check against) and only breaks the image build and CI's `--check` step, since both compile entirely
offline against the committed `.sqlx/` cache, with no live database to fall back on if it's stale.

## Profiles

| Profile | Auth | Notes |
|---|---|---|
| `dev` (or no profile at all) | `auth::dev::resolve` resolves `X-Manager-Id` once in the `authenticate` middleware, and *only* when the header is there — no header is an anonymous request, exactly as no bearer token is in `prod`, so a public route costs no manager lookup and anything gated needs the header; `CurrentManager`/`MaybeManager` just read the `Manager` back off the request extensions | plus `db/seed` applied via a dev-shaped Flyway invocation, so an admin manager (*NeonStrategist*, in the fixture) and the rest of the demo data actually exist |
| `test` | same dev stub | Testcontainers Postgres, one database cloned per test from a migrated template; same `db/seed` content, since every integration test asserts against the fixtures |
| `prod` | `auth::supabase::resolve` verifies the Supabase JWT, resolves `sub` → `managers.auth_user_id`, JIT-provisions, once per request | Needs `DB_URL`/`DB_USER`/`DB_PASSWORD`/`SUPABASE_JWKS_URI`, plus optional `FRONTEND_ORIGIN` (only for a frontend calling the API cross-origin instead of through the Worker proxy) |

`Config::is_prod` (`SPRING_PROFILES_ACTIVE` containing `"prod"`, split on commas — the variable name
did not change in the port, since it is the one both compose files and the VPS's hand-managed
`/opt/umfl/.env` already set) is the **only** thing that decides which of the two credential paths
`auth::authenticate` takes. It carries no route knowledge of its own — see `auth::authorize` below for
that. Which Flyway locations were applied to the database is a separate, orthogonal question decided
entirely by how the one-shot `flyway` container was invoked (see Commands above); the running server
process never reads `db/seed` or knows whether it exists.

The application makes exactly one outbound HTTP call, and only from the admin match import:
`matchimport::scraper::HttpScraperClient` → the scraper sidecar (see Match import below). Nothing
else in the codebase talks to the network.

There are no scheduled tasks and no background workers in any profile, with one narrow exception:
`standings::sse::StandingsSseHub` attaches a keep-alive to every open standings SSE stream so
idle-timeout proxies/browsers don't silently drop them. It's transport plumbing for the live standings
feed below, not a business-logic worker — it has no DB access and does nothing but write SSE comment
lines to already-open connections. It has no dedicated thread pool: axum attaches the heartbeat to
each response stream directly, and a slow client backs up in its own task and its own `broadcast`
receiver rather than behind anything shared. A subscription is released by `Drop` on the response
body.

**Which routes need an identity is decided in exactly one place, for every profile**: `auth::authorize`
(`crates/umfl-server/src/auth/authorize.rs`)'s `rules()` table, an ordered list of
`(method, ant-style path pattern, access)` rows that `authorize` walks first-match-wins. It allowlists
the read-only GETs that viewing a tournament needs — nobody needs an account to browse tournaments,
hero pools, standings or match history, only to enter and draft — and everything else under `/api/**`
is `Access::Authenticated`, with a trailing `/**` → `Access::Deny` backstop. Keep it in step with
`authorize_rules` in `tests/it/security.rs`, which asserts it from the outside. The two credential
paths (`auth::dev`, `auth::supabase`) differ only in how a credential is *verified*, never in which
routes require one — which is also why neither carries any route knowledge of its own; each resolves
an identity only when the request actually offered a credential, so no public GET pays a JWT
verification or a manager lookup. **A denial's status depends on who is asking**: an anonymous
request gets 401 ("you have not said who you are"), an authenticated-but-not-admin one gets 403
("you have, and it is not enough") — this is `authorize`'s own branch, not a framework default,
and both bodies render without an `instance` field.

`ratelimit::RateLimiter` is registered as middleware ahead of `authenticate` for every `/api/**`
route, an IP-keyed token-bucket throttle, so a flood doesn't pay JWT verification or the dev manager
lookup either. It keys on the peer address `into_make_service_with_connect_info` supplies by default,
because a forwarded-for header from a peer that could itself be the flooder is worthless.
`X-Forwarded-For` is read *only* when the peer falls inside `RateLimitConfig.trusted_proxies` —
loopback plus the RFC1918 ranges, which is what a TLS-terminating reverse proxy on the same VPS looks
like (note it arrives as the Docker bridge gateway, e.g. `172.17.0.1`, not `127.0.0.1`, even when the
container is published on `127.0.0.1:8080`). Without that carve-out a proxied deployment puts the
entire internet in one bucket. `RateLimiter::client_ip` reads the **last** forwarded entry, not the
first: a proxy appends the address it saw, so the trailing entry is the only one it vouches for, and
reading the first would let a flooder mint a fresh bucket per request with a fake prefix. A backend
exposed directly on a public interface never matches a trusted range and keeps the original
behaviour. The tradeoff moves one hop out rather than disappearing: traffic arriving through the
Cloudflare Worker still shares a bucket per Cloudflare edge IP rather than per visitor. The per-IP
bucket cache is bounded by `RateLimitConfig.max_tracked_ips` rather than an unbounded map, since the
key space is every IP that ever touches `/api/`. Tuning (`capacity`, `refill_period`,
`max_tracked_ips`, `trusted_proxies`) comes from the `RATE_LIMIT_API_*` environment variables via
`Config` — see the `umfl.*` invariant below for why this, alongside `scraper_base_url`, is one of the
only two things read from the environment rather than the database.

Admin routes (`/api/admin/**`) require `Access::Admin` — `manager.is_some_and(|m| m.is_admin)`, our
own data (`managers.is_admin`), never an identity-provider claim, so swapping auth providers later
never touches the role logic. There is **only one authorization layer here**: axum has no per-handler
annotation to forget, because every route is declared exactly once, in its own feature's `routes()`,
and merged into the single tree `auth::authorize` runs in front of. A feature handler that needs the
caller extracts `CurrentManager` for the value, but the *admission* decision already happened in the
middleware before the handler ever runs.

Adding a Supabase project also requires the Discord provider configured in the Supabase dashboard —
the backend never talks to Discord, only to Supabase-signed tokens.

## Backend architecture

`backend-rs/` is a two-crate Cargo workspace. `umfl-domain` is pure — `serde`, `chrono`,
`rust_decimal`, `indexmap`, `thiserror`, nothing that talks to a database or the network — and its
manifest enforces that at build-file level rather than by convention: adding an I/O dependency to it
is a compile-time question, not a review comment. `umfl-server` is `axum` + `sqlx` + all I/O, laid
out package-by-feature: one module per domain concept
(`hero`, `map`, `tournament`, `scoring`, `r#match`, `matchimport`, `standings`, `manager`), plus
`auth`, `http`, `error.rs`, `ratelimit.rs`, `state.rs` and `config.rs`.

**The read/write split is the central convention**, and it is kept by filename inside each feature
module rather than by class suffix: `query.rs` is the hand-written `sqlx::query_as!` read side,
`writer.rs` is aggregate writes, `pool_admin.rs` is writes to a composite-keyed link table
(`tournament_heroes`, `tournament_maps`), and `service.rs`/`admin_service.rs` are the transaction
boundaries — the file that calls `pool.begin()` is the file responsible for the whole unit of work, so
the boundary stays auditable file by file. There are no injected dependencies or
a DI container to mock: everything is a free `async fn` taking `impl PgExecutor<'_>` (composes inside
a transaction or outside one) or `&mut PgConnection` (part of somebody else's transaction) or, for a
service, `&AppState`. `matchimport::scraper::ScraperClient` is the one trait in the whole crate,
because it is the one genuine test seam (`tests/it/match_import.rs`'s `StubScraper` swaps it by
mutating `AppState`); reaching for a second trait is almost always reproducing dependency injection
rather than porting the read/write split. There is one cache — `r#match::cache::MatchResultCache`, an
in-memory read-through in front of `r#match::query::find_by_tournament` — and it exists only because
its key has a complete invalidation signal (see the standings section). A second one wants that same
argument made explicitly, not a precedent.

DTOs live with their feature module rather than in a shared file: it removes the worst merge contention
between people working on different features at once, without changing any JSON shape.
`umfl_domain::tournament::TournamentEntry`
is the manager-facing aggregate root, owning `EntrySlot`s by tournament-entry id; `managers` is
written on JIT-provisioning in `prod`. Everything else is written only through the Admin API, whose
own writers (`r#match::writer`, `scoring::writer`, `hero::writer`, `map::writer`) are described under
Admin API below; nothing outside that surface writes reference data or results.

### Invariants

- **No `season`, anywhere.** The tournament is the unit of scoping. Hero cost is
  `tournament_heroes.cost`; queries take a `tournamentId`.
- **No cost snapshot.** `entry_slots` stores only the hero; cost is joined live, so re-pricing a hero
  re-prices an *unlocked* roster. That is intended. What *is* snapshotted is
  `tournament_entries.credit_grant`, copied off the tournament at registration —
  `roster_policy::validate_lock` takes the budget from the entry, never the tournament.
- **Nothing writes points.** Match results are written by the Admin API (see below), but every
  point total is still derived at read time by `umfl_domain::standings::board`/`ticker`, called from
  `standings::service`; total cost is derived from slots. Do not materialise either.
  `MatchResultCache` is not a loophole in that: it holds the fold's *input* — the assembled match list
  — in memory, never its output, and the asymmetry is the point. Matches have exactly one writer and
  so a complete invalidation signal; a total would depend on coefficients and costs that are retuned
  with a bare UPDATE and announce nothing. So the rules, rosters and prices behind a board stay read
  live on every request, and only a *match* can ever be a write behind.
- **There are no `umfl.*` configuration properties.** Scoring weights are rows in
  `scoring_coefficients`, the budget is `tournaments.credit_grant` — both retuned with an UPDATE. Don't
  reintroduce a tunables block. There are exactly two exceptions, and both are *infrastructure* rather
  than domain data — the same category as `DB_URL`, and unreadable from a database the process hasn't
  reached yet: `RATE_LIMIT_API_*` (`RateLimitConfig`, see Profiles above), operational tuning for how
  hard a deployed instance throttles per client IP; and `SCRAPER_BASE_URL` (see Match import below),
  the address of the scraper sidecar. Neither belongs in the database the way a scoring weight or a
  budget does. A third would want the same justification, not a precedent —
  `standings::sse::MAX_SUBSCRIBERS_PER_TOURNAMENT`/`MAX_TOTAL_SUBSCRIBERS` and `MatchResultCache`'s
  sizing are plain Rust constants for exactly that reason: how many match lists or SSE subscribers a
  process holds is neither domain data nor deployment topology.
- **A match names where it is recorded elsewhere, and that link is its identity.**
  `tournament_matches.external_link` is `not null` with a unique index per tournament
  (`uq_tournament_match_external_link`) — folded into the `V1` baseline by the most recent squash;
  historically it arrived as a dedicated `V9__external_link_required.sql` migration, which no longer
  exists as a separate file for the same reason `V6__demo_draft_picks.sql` and
  `V8__demo_ban_sides.sql` don't (see Commands above). It is what stops an admin importing the same
  match twice — a duplicate would silently double every point the match scores, and nothing would
  surface it until someone doubted the standings. So it is required even for a match typed by hand: a
  game with no page anywhere carries an identifier of the admin's own, and rows predating the original
  migration were backfilled with a synthetic `urn:umfl:match:<id>`. The uniqueness is scoped to the
  tournament rather than global, matching the importer's own per-tournament check, so the preview
  never reports "no duplicate" and then fails the save. Correcting a match reuses its link freely —
  `r#match::admin_service::correct` updates the row in place.
- **A match is a series, and every game in it has a winner.** `tournament_matches` is a best-of-N
  between two humans; each `match_games` row carries its own map and its own two
  `match_game_participants` rows, so a side can pilot a different hero per game. Exactly one of those
  two rows is flagged `is_winner` — a partial unique index stops two, and
  `MatchRule::NotExactlyOneWinner` (`umfl_domain::match_policy`) stops zero. **There is no draw**, and
  the loser never survives: `MatchRule::LoserHasPositiveHealth` requires the losing side to finish on
  0 or less (an overkill hit lands it below zero), and every recorded game in `V3__demo_fixtures.sql`
  respects that. Nothing stores who won the *series*: the admin frontend counts games won client-side,
  like every other derived number here.
- **The draft is recorded in full, as picks *and* bans, and both name a side.** `hero_bans
  (match_id, hero_id, ban_type, side)` holds the heroes struck out of a series;
  `match_hero_picks (match_id, side, hero_id)` holds the heroes each side took. Both are per series,
  never per game. A recorded draft is *complete* — `MatchRule::PlayedHeroNotDrafted` rejects a game
  whose hero is missing from that side's picks — which is what lets `APPEARANCE` be "was drafted and
  not banned" rather than "played". `MatchRule::BannedHeroDrafted` keeps the two halves disjoint.
  There is deliberately no `unique (match_id, hero_id)` on the picks: games are independent, so a hero
  may legitimately go to one side in game 1 and the other in game 2, and
  `MatchResult::drafted_hero_ids` (`umfl_domain::match_result`) de-duplicates instead. `hero_bans`
  *does* keep that key, so a hero is struck at most once per series however many sides wanted it —
  which is what `MatchRule::DuplicateBan` means.
- **A ban's `side` is the draft it came out of, not who struck it.** `ban_type` already says that:
  `SELF_BAN` is a side striking one of its own, `OPPONENT_BAN` the other side striking it, and a
  `PRE_BAN` precedes side assignment and so carries no side at all (`MatchRule::BanSideInvalid`
  rejects one that does). The column is **nullable and stays that way**: rows written before it
  existed have no side and cannot be given one after the fact, so a typed ban without one is legal
  rather than making an already-recorded match uncorrectable — `BanSideInvalid` polices an
  *impossible* side, never a missing one. The column arrived as a dedicated `V7__hero_ban_side.sql`
  migration, since folded into the `V1` baseline like `V9__external_link_required.sql` above;
  tightening the invariant to "every typed ban names a side" still needs a migration that can first
  attribute the rows already in the table, and cannot be done by editing the baseline, which fails
  Flyway checksum validation. Scoring never reads the column: `umfl_domain::match_metrics`'s ban
  extractors price a ban by category alone, because points are per hero and never per player.
- **No `player` entity.** Every point is scored per *hero*: no metric extractor, no coefficient and
  no standings query reads the human who piloted it. So the competitor is
  `match_participants.player_label` — one row per side for the whole series, nullable free text with
  no table, no FK, no repository and deliberately no admin API. An admin records a new competitor by
  typing their name. It is display text for the ticker and the admin match list, nothing more, and
  `umfl_domain::match_policy::validate` never validates it (a blank label normalises to `None` in
  `r#match::admin_service`). Promote it to a real table only if something starts scoring or ranking
  the humans — until then, a `player` table only buys you CRUD you have to build and a foreign key
  that can 500.

Domain rules live in pure functions in `umfl-domain`, with no `sqlx`/`axum`/I/O dependency at all —
`roster_policy`, `match_policy`, `match_metrics`, `scoring_engine`, `scoring_rule_set_policy`. New
rules belong there, tested directly, not inside a `service.rs`.

`roster_policy::validate_draft` deliberately permits over-budget selections (the builder is a
scratchpad, the meter just runs past 100%); `validate_lock` adds the budget and roster-size checks.
Both return *all* violations at once so the UI can highlight every problem in one pass. The rule
codes are the `RosterRule` enum, documented constant by constant.

`match_metrics` is a registry keyed by the free-form `scoring_coefficients.metric` string. It
implements `APPEARANCE`, `SELF_BAN`, `OPPONENT_BAN`, `WIN`, `LOSS`, `HEALTH_REMAINING`,
`HEALTH_DIFFERENTIAL`, `HEALTH_DIFFERENTIAL_TWO_WAY`, `SHUTOUT`, and **silently ignores everything
else** — unknown keys score zero, are dropped from the leaderboard's columns and error nothing. The
seed's `CROWD_FAVOURITE` is the deliberate proof of that; leave it unimplemented. There is
deliberately no `DRAW`: every game has exactly one winner (see the invariant above), so `WIN` and
`LOSS` are exhaustive within a game and a `DRAW` column would price something that cannot be
recorded. Extractors take a `MetricContext` (`umfl_domain::match_result` — the hero's role in one
match: `Played`, scoped to *one game* of it, or the per-series `Drafted`/`Banned`), not a bare
participant row, because `HEALTH_DIFFERENTIAL` needs the opponent and
`APPEARANCE`/`SELF_BAN`/`OPPONENT_BAN` have no participant row at all — they price the draft, reading
`hero_bans.ban_type` off the match or the role itself, so a hero banned `PRE_BAN` (struck before sides
are known) scores neither ban metric. `HEALTH_DIFFERENTIAL` is also win-gated: a hero that did not
win the game scores 0.0 rather than a negative differential, since there is no losing side of that
metric to price. `HEALTH_DIFFERENTIAL_TWO_WAY` is the ungated half of that pair and the *only*
difference between them — same gap (both share a private `health_gap` helper), but the loser scores
its negative, so a heavy defeat costs what a clean victory earns. They are two registry keys rather
than one key with a flag because a rule set is a set of weighted metric rows: an admin picks the
behaviour by naming it, and pricing both is legal. Don't collapse them back into one extractor.
`WIN`/`LOSS` are scored per game, not per series, so a hero that takes game 1 and drops game 2 of a
Bo3 collects one of each.

`MatchResult::hero_contexts()` (`umfl_domain::match_result`) is where per-game and per-series part
ways, and the split is the whole reason `APPEARANCE` is not multiplied by series length: a hero that
played yields one `Played` context *per game* plus exactly one `Drafted` context, a hero drafted and
never fielded yields only the `Drafted` one, and a banned hero yields only `Banned`.
`umfl_domain::standings::ticker` has to bridge that, since its rows are games: it banks the `Drafted`
context against the hero's **first** game so the ticker's per-game points still sum to what the board
gained, and names the never-fielded picks separately.

`umfl_domain::standings::board` returns a `StandingsBoard` that carries its own `MetricColumn`
definitions — the backend cannot know the columns until it reads `scoring_coefficients`. Ranking is
standard competition ranking (1, 2, 2, 4), computed by the private `rank()` in the same module. The
ticker's polling key is **`sinceMatchId`** (monotonic `bigserial`), never `playedAt`: parallel tables
in a round share a timestamp.

`GET /api/tournaments/{id}/standings/stream` is an SSE endpoint (`standings::sse::StandingsSseHub`)
that pushes a bare "something changed" event after `r#match::admin_service::record`/`correct`/`delete`
commits. The event carries no board/ticker payload — the frontend already knows how to pull fresh
data via the existing `/standings` and `/matches` endpoints, so the stream is purely a "poll now"
signal, not a second copy of the data.

That push is also what makes the read path bursty, and `MatchResultCache` (`r#match::cache`) is the
answer to it: one write tells up to 200 tabs per tournament to refetch, and each pulls *both* the
board and the ticker head, so uncached that is hundreds of simultaneous runs of
`r#match::query::find_by_tournament`'s six unbounded queries against a ten-connection pool. The cache
is a read-through front for that query — `standings::service` calls `find_by_tournament`/
`find_by_tournament_since` on the cache, never on the query directly — and moka's `try_get_with` is
atomic per key, collapsing the burst onto one load. **It holds the fold's input only**; see the
"Nothing writes points" invariant for why a cached `StandingsBoard` would be the thing with no
complete invalidation signal. The ticker's page is sliced out of the same cached list rather than
requeried: `(played_at, id)` is a total order, so reversing the ascending list *the database ordered*
is exactly the ticker's `desc` — which is why `find_by_tournament_since` survives with no production
caller, as `tests/it/match_cache.rs` checks the slice against a SQL oracle.

Three details there are load-bearing and easy to undo by accident. **Every write announces
invalidation twice, at two different moments**: `r#match::admin_service`'s `record`/`correct`/`delete`
each call `announce` before the transaction commits (so the writer sees its own effect immediately)
and `announce_completed` after the transaction *ends*, on **commit or rollback both** — a rollback
un-writes rows the cache may already have loaded inside that transaction, and so invalidates just as
surely as a commit. `announce_committed`, the third call at each site, fires **only** on commit, and
is what `StandingsSseHub` listens to — telling browsers "something changed" about a rolled-back write
would be a lie, so the SSE push and the cache invalidation deliberately fire on different signals.
**`invalidate` bumps a per-tournament version and deliberately does not evict** — a bumped version
already means no reader accepts the entry, and an eager evict would have to take the key's lock,
putting an admin's write behind a stranger's in-flight query. That version stamp is what closes the
invalidate-during-load race (a reader that misses, loads across a write, and would otherwise store a
stale list nothing ever invalidates again — the stamp is read *before* the load starts, so a spurious
mismatch costs one extra load rather than swallowing a real invalidation). And **there is no TTL**: an
expiry cannot add a guarantee the stamp does not already give, and would only turn a missing hook into
an intermittent bug. A hero or board **rename** is the one staleness a match write cannot announce,
since `hero_name`/`map_name` are copied into an assembled `MatchResult` — `hero::admin_service::update`
and `map::admin_service::update` call `state.match_cache.invalidate_all()` directly when the name
actually changed, the same two-phase treatment a match write gets. `tournament::admin_service::delete`
needs no hook: every standings route calls `require_tournament` and 404s first.

Handlers take the caller via the `CurrentManager`/`MaybeManager` extractors (`crate::auth`); a route
that permits anonymous access takes `MaybeManager` instead. Errors go through `ApiError`
(`crate::error`) as RFC 7807 problem details: `DomainError` (`umfl_domain`) covers the six domain
exceptions with their status mapping, and `RosterRule`/`MatchRule`/`ScoringRule` violations all render
422 with a `violations` array. Convert into one of `ApiError`'s existing variants rather than adding
a bare status code — every variant is defined once, in `error.rs`, precisely so a feature never has
to decide its own error shape. Every error this API produces is an RFC 7807 document; there is no
bare status code or plain-text body anywhere in a handler.

Never use a bare `axum::Json`, `Path` or `Query` as a request extractor in a handler — use
`http::extract::{AppJson, ValidJson, AppPath, AppQuery}` instead. A bare extractor rejects with
axum's plain-text body and no problem type, which is a wire-contract break; the wrappers reject with
the same RFC 7807 shape every other error does. (`axum::Json` as a **response** type is fine — the
rule is about the request side.) This is grep-able on purpose:
`grep -rn 'axum::Json<\|Json(\w*): *Json<\|Path(\|Query(' crates/umfl-server/src --include='*.rs'`.

`hero::query::HeroSort` holds ORDER BY fragments as an enum whitelist because sort keys can't be
parameterised — keep new sorts inside that enum.

Migrations are `db/migration/V*__*.sql` at the **repository root**, forward-only, squashed to a single
`V1__core_schema.sql` baseline (see Commands above for why, and for the seed's separate Flyway
location). Tables the app loads or writes as aggregates carry a surrogate `bigserial` id next to a
unique natural key; the pure link tables (`tournament_heroes`, `tournament_maps`, `entry_slots`,
`hero_bans`, `match_hero_picks`, `match_game_participants`) keep natural composite keys and are read
and written through hand-written SQL rather than an aggregate mapper. The integration tests assert on
the seed's numbers exactly — changing a seeded price or result means updating
`V3__demo_fixtures.sql` and the tests together. New *league* data past that fixed baseline goes
through the Admin API (see below), not another migration. The one thing that legitimately arrives as
a migration is reference data: a hero or a board Restoration Games releases later belongs in a
forward migration alongside `V2__reference_data.sql` (or through `/api/admin/heroes`/`/api/admin/maps`,
which write the same tables), since a `prod` database has to have it without anyone hand-entering
74 heroes.

## Frontend architecture

Vue 3 `<script setup>` + Pinia setup-stores + Vue Router, Tailwind v4. `@/` aliases `frontend/src`.

`src/api/client.ts` is the only place that calls `fetch` — it attaches the Supabase bearer token,
unwraps RFC 7807 bodies into `ApiError` (with `.violations` for 422s), and exposes a typed `api`
object. Add endpoints there, with matching types in `src/api/types.ts` — that file is the contract
anchor, so change it first and let `npm run type-check` point at every consumer. The backend
serializes nullable response fields with `skip_serializing_if = "Option::is_none"`, so a nullable
field is *absent* on the wire, not `null`; templates need a `?? '—'` fallback (e.g. `mapName`).

Stores: `auth` (Supabase session), `manager`, `heroes` (keyed by tournament — cost is
tournament-scoped), `tournaments`, `roster`, `standings`. `roster` keeps an optimistic `selectedIds`
and rolls it back if the server rejects. `standings` loads once via `load(id)`, then opens the
`/standings/stream` SSE connection (`src/api/sseClient.ts`) and calls `refresh()` every time the
backend signals that a match was written. `refresh()` always refetches the full ticker head from
`sinceMatchId=0` rather than incrementally — a correction reuses an existing match id and a deletion
removes one, so an incremental "since" fetch could miss either. There's still no client-side polling
timer; the server-pushed event is what triggers each `refresh()`. The stream is closed and reopened
on tournament switch and closed on unmount.

`src/domain` is this side's answer to the backend's pure `umfl-domain` functions: plain functions over
plain data, no Vue import, tested as data rather than through a `mount()`. A rule that a component can
state without a DOM belongs there. Two modules live in it.

`src/domain/rosterPolicy.ts` intentionally duplicates the Rust budget arithmetic
(`roster_policy::budget_status`) so the meter reacts on click. **If you change that arithmetic, change
both sides** — `crates/umfl-domain/src/roster_policy.rs` and `rosterPolicy.ts` — and their tests. The
server stays authoritative.

`src/domain/matchForm.ts` is `MatchResultWizard.vue`'s whole domain half — the `MatchForm` model,
the seeding (`formFromPreview`, `formFromMatch`), the payload conversion (`toPayload`), the dropdown
option lists, the edits that cascade (`removeDraftPick`, `removeGame`, `assignBanToSide`,
`setWinner`) and `validate`. It has no counterpart on the backend the way `rosterPolicy.ts` does: it
is not a mirror of a server rule but the arithmetic that keeps this one *form* consistent with the API
it posts to. The wizard and each of its section components import it as `matchForm.*`, so a
`matchForm.` call site marks a rule and anything without one is rendering. Tests split the same way — `matchForm.spec.ts` is data-in/data-out
and owns every rule, `MatchResultWizard.spec.ts` mounts only to pin the wiring between the two.

Routes: `/lobby`, `/standings` and `/login` are public; `/tournaments/:tournamentId/roster` requires
a session plus a `beforeEnter` guard that bounces to the lobby unless the manager has an entry or
the tournament accepts registration. `/admin` (`AdminDashboardView.vue`) has a `beforeEnter` guard
that bounces non-admins to the lobby, and `AppShell.vue` only renders the nav link when
`manager.isAdmin` — both are UI convenience, not the security boundary, since every Admin API call is
still `Access::Admin`-gated server-side.

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
from the active rule set and isn't knowable in advance. Rank, Manager **and Total** stay pinned
while it scrolls — the total is the number the page exists to report, so it sits beside the
identity rather than past the whole breakdown, where a phone could only reach it by scrolling. The
round's own gain rides as a sub-line under the total instead of a `Last Rd` column of its own. That
costs a few non-obvious rules in `main.css` — each pinned column's width is the offset the next one
pins at, so the chain derives from two shared custom properties, and a cell's content has to stay
inside its declared width or every offset after it drifts. They're commented where they live, next
to `.cell-pinned`.

`e2e/responsive.spec.ts` guards all of this from the `mobile` Playwright project (Pixel 5): it
asserts `document.documentElement.scrollWidth` never exceeds the viewport on any route, which is the
failure mode that matters — nothing sets `overflow-x` on `body`, so one over-wide element slides the
whole page. Like the rest of the e2e suite it needs a live backend and is not part of CI.

`frontend/.env.local` needs `VITE_SUPABASE_URL` and `VITE_SUPABASE_ANON_KEY` (see `.env.local.example`),
plus optional `VITE_DEV_MANAGER_ID` to skip Supabase Auth against a dev backend.

## Admin API

`/api/admin/**`, `Access::Admin`-gated, backed by `managers.is_admin` (our own data, independent of
any identity provider). Covers create/update for tournaments, heroes, maps, per-tournament hero
pool/pricing (`tournament_heroes`), per-tournament board pool (`tournament_maps`), and scoring rule
sets/coefficients, plus create/update/delete for match results. Both pools also support removal, and
the two removals are deliberately asymmetric: dropping a hero from `tournament_heroes`
(`hero::pool_admin::remove_from_pool`) is always allowed and simply re-prices any roster still holding
it to 0 (the "no cost snapshot" invariant above, applied to a removal rather than a re-price), while
dropping a map from `tournament_maps` (`map::admin_service::remove_from_pool`) is rejected with
`DomainError::conflict` when the tournament has a recorded game on it, since `match_games` carries a
composite FK onto that row. That FK is `DEFERRABLE INITIALLY DEFERRED` so a tournament delete (which
cascades to `tournament_maps` and, one level deeper, to `match_games`) is not tripped by cascade
ordering — which is why `map::admin_service::remove_from_pool` calls
`map::pool_admin::check_map_in_pool_now()` (`set constraints … immediate`) after its DELETE: the
violation has to surface inside the function that can still name the map, not at commit.

`hero`, `map`, `r#match` and `scoring` each own a `writer.rs` that inserts/updates the aggregate with
plain `sqlx` statements — there is no ORM anywhere in this crate, so there is no distinction between
what an ORM could map automatically and what needs hand-written SQL; every write is hand-written, and
the read/write split (query.rs vs. writer.rs vs. pool_admin.rs) is a convention rather than a tooling
constraint. A `TournamentMatch`'s participants, games, bans and picks
are all written inside `r#match::writer`'s one transaction. The *API* still hangs the draft off the
side that owns it — `MatchParticipantRequest.drafted_hero_ids` in, `MatchParticipantResult
.drafted_heroes` out (`side` is the position in `participants`, as it already is for `player_label`) —
and `r#match::admin_service::to_picks` does the transposition into per-side pick rows, which is also
why an out-of-range side is unrepresentable and no rule polices one.

`umfl_domain::match_policy::validate` (mirrors `roster_policy`) validates a match submission before
save, raising a `Vec<MatchViolation>` the service converts into `ApiError::Domain` at the boundary
(422, same shape as a roster rule breach). The checks are the `MatchRule` enum, one doc line each —
read them there. Two enforce the no-draw invariant above and are the ones to know before touching the
policy: `NotExactlyOneWinner` treats zero winners as being as wrong as two, and
`LoserHasPositiveHealth` rejects a loser who survived. Three more police the draft:
`PlayedHeroNotDrafted` is what makes a recorded draft complete (and so what makes `APPEARANCE`
measurable), `BannedHeroDrafted` keeps picks and bans disjoint, and `DuplicatePick` mirrors
`DuplicateBan`. `BanSideInvalid` polices the ban side described in the invariants above. Activating a
scoring rule set deactivates any active sibling in the same transaction, since only one may be active
per tournament. An unknown scoring metric (e.g. the seed's `CROWD_FAVOURITE`) is surfaced as a
non-blocking warning on the response, never rejected.

`umfl_domain::scoring_rule_set_policy::validate` (pure, same pattern again) validates a rule set's
coefficients on create and update, with rule codes `DUPLICATE_METRIC` and `MALFORMED_METRIC`,
converted into the same 422 shape. Both checks run against the *normalised* metric, so `' win '` and
`'Win'` are a duplicate of each other, and `MALFORMED_METRIC` mirrors the schema's
`^[A-Z][A-Z0-9_]*$` CHECK. Without it a duplicated or hyphenated metric reached the database and came
back as `ApiError::DataIntegrity`'s generic 409, which names nothing. The policy validates the *shape*
of a metric name and never the *set* — an unimplemented metric stays a warning, per the paragraph
above.

## Match import

`POST /api/admin/tournaments/{id}/matches/import` (`matchimport::mod::routes` → `matchimport::service
::preview`) turns a tabletopleague.com match URL into a reviewable draft, so an admin doesn't re-type
a result the source site already has.

**The endpoint writes nothing.** It scrapes, resolves names to ids, and returns a
`MatchImportPreview`; the client fills the gaps and submits through the ordinary record endpoint.
That is the whole design — an imported match goes through `match_policy::validate` exactly as a typed
one does, and the importer never needs to know the result rules. `tests/it/match_import.rs` pins it
by feeding a preview straight back into `POST .../matches` and asserting a 201; that test is the seam
that fails if the importer ever starts emitting something the policy rejects.

**Why a sidecar.** tabletopleague.com is client-rendered Next.js: fetching a match page from the
backend returns the JS bundle and no data, so a real browser has to render it. That browser is
`tools/tabletopleague-scraper/server.mjs` — a Node/Playwright HTTP wrapper around the same
`scrapeOne` the CLI uses, so there is no second extractor to keep in step. The backend reaches it
with a plain HTTP client at `SCRAPER_BASE_URL`. It is *not* in the backend's own image, since running
a real browser is a different deployment shape entirely, and the backend does not depend on it at
startup: a scraper that is down costs the import endpoint a 503
(`DomainError::service_unavailable`, message naming the address and how to start it) and nothing
else.

**Three things a scrape cannot supply**, by nature rather than by omission: the `tournamentId` (a
path variable), the `round` — the source names its pools ("The Wayward Sisters") where the schema
wants a positive integer, so the preview carries `roundName` as context and the admin types a number —
and hero/map **ids**, since the site only ever has names. A fourth is conditional: `playedAt` is
parsed best-effort by `umfl_domain::scraped_timestamps::parse`, which returns **`None` rather than
erroring** on a timezone abbreviation it can't resolve unambiguously. Losing a whole scraped match
over one rendered timestamp would be a bad trade; the wizard's date picker already defaults to now.

**A ban's side is scraped, not inferred.** The source groups its "Self ban"/"Opp. ban" chips under
the side that owned the hero (`ScrapedSide.bans`), and `scraped.pre_bans` belongs to neither — so
`matchimport::service` emits each typed ban's side straight through to `hero_bans.side`. It used to
flatten both sides into one list and throw the attribution away, because the table had nowhere to put
it. The flattening remains, since `hero_bans` is keyed `(match_id, hero_id)` and the same hero cannot
be struck twice in one series; only the discarding is gone.

**Name resolution is exact-after-normalisation, with no fuzzy fallback** (`umfl_domain::name_resolver`,
pure). Normalisation covers the drift actually seen — case, whitespace, the `.` in `Dr. Ellie Sattler`,
`&` versus `and` — and nothing more. A near miss that silently resolves to the wrong hero scores
points for the wrong manager and is invisible until someone doubts the standings; an unmatched name
is merely reported. Don't turn this into an alias table: a genuinely differently-named hero is a
catalogue question.

Anything unresolved comes back in `unresolved[]` with the source's own spelling and never blocks the
*import* — only the recording. `MAP_NOT_IN_POOL` is the one that fires in practice, because
`match_games` carries a composite FK onto `tournament_maps`; heroes reference `heroes(id)` directly and
have no equivalent constraint. `external_link` stores the source URL, which is also the duplicate
check — the column is `not null` with a unique index per tournament,
`matchimport::service::preview` reports the clash as `already_imported_match_id`, and
`r#match::admin_service` refuses the write with `DomainError::conflict` naming the match to correct. A
correction still reuses the URL legitimately — `correct` re-saves the same aggregate root, so the row
is updated in place and never meets the index; only moving one match's link onto another's conflicts.

## Admin frontend

`/admin` (`AdminDashboardView.vue`) is a manager-gated dashboard, not a separate app — it composes
per-entity wizard components (`TournamentManagementWizard`, `HeroManagementWizard`,
`MapManagementWizard`, `HeroPoolWizard`, `MapPoolWizard`, `ScoringRuleSetWizard`,
`MatchResultWizard`, `MatchListAdmin`) that each call the corresponding `/api/admin/...` endpoints
through the same `src/api/client.ts`. Those are the dashboard's own children; only
`MatchResultWizard` is itself composed further, out of the four section components below. It's
reachable only by managers with `isAdmin` true — see
Routes above for the two layers of UI gating — with the Admin API's own `Access::Admin` check as
the actual security boundary.

`ScoringRuleSetWizard` is the one place the "unknown metrics are a warning, not a rejection" rule
becomes visible. It lists a tournament's rule sets with their active flag, edits and activates them,
and **renders `ScoringRuleSetDto.warnings` after every save** — without that, a mistyped metric is a
clean `201` followed by a column that silently scores zero forever. Its `knownMetrics` array mirrors
`match_metrics`' extractor registry and only drives a hint (an inline flag while typing, normalised
the same `trim().uppercase()` way); it never blocks a save, because the server deciding what it can
price is the whole point of the warning. Keep the array in step when adding an extractor — and note
`DRAW` is *not* in it, deliberately, so pricing one is flagged like any other metric this build
cannot measure.

`MatchResultWizard` records a whole series, and **the draft comes first**. Each side's block asks for
its whole arsenal — every hero it took, whether that hero played, sat out, or was struck by a ban —
and every hero dropdown below is filtered to it: a game's "Side N Hero" offers only that side's
drafted-and-unstruck heroes, and a side's ban rows offer only its own arsenal. That filtering is what
makes `PLAYED_HERO_NOT_DRAFTED` and `BANNED_HERO_PLAYED` unreachable from this form, so the wizard
does not check for them. Games follow: one map and two heroes each, "+ Add Game" for a best-of-N,
with the winner a **radio** rather than a checkbox — exactly one side wins, and there is no "neither"
to express — plus a client-side check so an untouched winner reads as a prompt instead of a 422.
Pre-bans come last, in their own section, offering only heroes neither side drafted.

All of that lives in `src/domain/matchForm.ts` (see Frontend architecture above), not in the SFC —
the component holds the reactive `form` ref, the template and the API calls, and delegates every
rule.

The template itself is split one section per part of a series:
`MatchUnassignedBanSection`, `MatchDraftSide` (one per side, so twice),
`MatchGameRow` (one per game) and `MatchPreBanSection`. `MatchResultWizard` keeps
what no section owns — the match's own fields, the pools it loads, `validate`, the save, and
"+ Add Game", which is the one edit that is nobody's row. Each section takes the **whole form** as
its `defineModel`, not its own slice, for two reasons that are worth not undoing: the option lists
are form-wide rules (a pre-ban has to know both drafts, a side's ban rows have to know that side's
games), so a section holding only its slice could not ask `matchForm` anything; and a `defineModel`
is a ref rather than a prop, so a section may edit the form in place — `v-model` on a prop's member
is what `vue/no-mutating-props` exists to stop, and threading every keystroke back up as an emit
would buy nothing here. The rules still live in `matchForm.ts`: a section's script is its option
lists, its plain pushes and splices, and nothing else. Tests follow the same line —
`MatchResultWizard.spec.ts` mounts the whole tree with the sections real rather than stubbed, and
the sections have no specs of their own, because a template over already-tested functions has
nothing left to assert alone.

The one real conversion is at save. The form holds the arsenal; the API wants
`draftedHeroIds` *without* the banned heroes (`BANNED_HERO_DRAFTED` keeps picks and bans disjoint),
so `toPayload` subtracts each side's bans back out and emits them as `bans` carrying their `side`.
`formFromMatch` and `formFromPreview` do the union in the other direction — keep the three in step,
which `matchForm.spec.ts` guards by running a saved match and a preview through both.
`removeDraftPick` cascades: dropping a hero off an arsenal also clears the games and ban rows that
named it, since a select holding an id no longer in its option list renders blank while still
submitting.

A ban read back with **no side** (anything recorded before `hero_bans.side` existed) lands in
`unassignedBans` and blocks the save until the admin places it on a side. It is neither dropped nor
guessed: the ban already scored points, so losing it would silently move the standings.

Its client-side checks belong in `matchForm.validate`, which the component wraps in a computed so a
banner clears as the admin types rather than standing until the next save. What is left in it is what
no single dropdown can see because it spans two: `DUPLICATE_BAN` across both sides plus the pre-bans,
and a pre-ban naming a hero someone drafted afterwards.

`MatchListAdmin` renders the games grouped under their maps, derives the games-won tally per side,
and under each side names both its drafted-but-unfielded picks — the heroes that scored an appearance
without appearing in any game row — and the heroes struck out of that side's draft. Only pre-bans,
and any ban still missing a side, keep a column of their own.

`MatchImportPanel` is the front of the Match import flow above: a URL field, then the scrape's
summary, its unresolved names, and a duplicate block. "Review and record" is disabled until
`unresolved` is empty **and** `alreadyImportedMatchId` is absent, and emits the preview up to
`AdminDashboardView`, which reopens `MatchResultWizard` in `create` mode with a `prefill` prop. An
already-imported URL instead offers "Open match #N to correct", which emits `correctExisting` and
lands on the same `startMatchEdit` the match list uses — the admin is sent to the existing match
rather than discovering the conflict after filling the wizard in. Two details are load-bearing. A
`MAP_NOT_IN_POOL` row carries an **"Add to board pool"** button that calls the existing
`addMapToPool` and re-imports — the admin widens the pool by clicking, the importer never does it
silently. And the wizard's `formFromPreview` **unions each side's bans back onto its drafted list**:
the preview splits them the way the API does, while the form holds the arsenal whole, so copying the
preview's `draftedHeroIds` in raw would drop every hero that side lost to a ban off the screen.
`formFromMatch` does the same union reading a saved match back — keep the two in step.

## Deliberately not built

A Hero Encyclopedia / Stats Lab (third-party sites already publish Unmatched stats — `heroes` is only
`(id, name, image_url)` on purpose).

## CI/CD

The root `docker-compose.yml`'s `backend` service builds `backend-rs/Dockerfile`, and
`deploy/docker-compose.prod.yml`'s `backend` pulls `ghcr.io/<owner>/umfl-backend-rs:latest` — it is
the only backend either compose file runs. Since the server deliberately carries no migration runner
of its own (see
`backend-rs/crates/umfl-server/src/config.rs` — Flyway is still "the migration mechanism", just not
embedded in the server process), both compose files carry a `flyway` service: a tiny image built from
`db/Dockerfile` that bakes `db/migration` and `db/seed` into the stock Flyway CLI image and runs
`migrate` once, gated by `depends_on: condition: service_completed_successfully` ahead of `backend`.
The root compose file's `flyway` uses both locations; the prod one uses `db/migration` only —
`db/seed` is demo/dev fixture data and must never touch the real league's database.

GitHub Actions. `.github/workflows/backend-ci.yml` is the backend merge gate: `cargo fmt --all
--check`, `cargo clippy --workspace --all-targets -- -D warnings`, a check that the committed `.sqlx/`
query metadata is current, then `cargo test --workspace` (Testcontainers included —
GitHub-hosted runners have Docker preinstalled) on PRs and pushes to non-`master` branches.
`.github/workflows/backend-deploy.yml` re-runs that test suite, then on a green push to `master`
builds and pushes `ghcr.io/<owner>/umfl-backend-rs:{sha,latest}` from `backend-rs/Dockerfile` *and*
`ghcr.io/<owner>/umfl-migrator:{sha,latest}` from `db/Dockerfile`, then SSHes into the VPS to pull and
`up -d` `flyway` and `backend` (in that order, so a freshly pulled migrator image is what actually
runs) against `deploy/docker-compose.prod.yml` — which the VPS keeps a copy of at `/opt/umfl`, alongside
a `.env` — modeled on `deploy/.env.example` — that is managed by hand on the box, never passed through
CI. The `prod` profile there talks to Supabase Postgres directly (over the internet, not a compose
network), so there is no `db` service in that compose file, unlike the root `docker-compose.yml` —
`flyway`'s `${DB_URL}`/`${DB_USER}`/`${DB_PASSWORD}` come from that same `.env` via compose variable
substitution, reusing the JDBC-shaped `DB_URL` `backend`'s own `env_file` already reads (the Rust
process converts it to a libpq-shaped URL itself — see `config::to_libpq_url` — so the VPS's `.env`
never had to change format across the cutover). That compose file also runs `cloudflared` as a
service (`tunnel run`, authenticated by `CLOUDFLARE_TUNNEL_TOKEN` in `.env`), publishing `backend` to
the internet as a named Cloudflare Tunnel rather than an ad-hoc `cloudflared tunnel --url ...` quick
tunnel run by hand on the box. A named tunnel's hostname is stable across restarts and redeploys — it
comes from the tunnel's own identity, configured once in the Cloudflare Zero Trust dashboard, not
minted fresh each time the process starts — which is what lets `BACKEND_HOST` below be a plain
committed value instead of a dashboard-only secret. `backend` itself only `expose`s :8080 on the
compose network rather than publishing it to the host, since `cloudflared` is the only thing that
needs to reach it now.

`.github/workflows/scraper-deploy.yml` builds and publishes the `tabletopleague-scraper` sidecar
image on its own, decoupled from `backend-deploy.yml` so publishing a scraper change doesn't also
trigger an unconditional SSH redeploy of the backend. It has no test job of its own — the sidecar has
no test script, and `backend-ci.yml` exercises the backend that calls it, not the scraper itself.

`.github/workflows/frontend-ci.yml` runs on `frontend/**` changes — `npm ci`, `npm run lint`,
`npm run type-check`, `npm test` (vitest) — as that side's merge gate; it does not build or deploy
anything. Deployment of the frontend stays separate: Cloudflare Pages is connected directly to the
GitHub repo and builds `frontend/` on every push and PR (its own preview-deployment mechanism,
running `vue-tsc -b && vite build`, no tests), independent of the backend pipeline — the two sides
deploy independently since neither needs the other to be green. The Playwright e2e suite
(`frontend/e2e/`) needs a live backend and Postgres and is not wired into either workflow yet.

`frontend/src/worker.ts` is what routes the deployed frontend to the backend. The frontend deploys as
a **Cloudflare Worker with static assets** (`frontend/wrangler.toml`), not a Pages project, so
`public/_redirects` would never be read — the Worker proxies `/api/*` to `env.BACKEND_HOST` by hand
and serves everything else from the `ASSETS` binding, falling back to `index.html` so Vue Router's
history mode survives a deep link. The effect is the same same-origin proxy: the frontend's relative
`/api/...` calls (`client.ts`, `sseClient.ts`) reach the VPS with no cross-origin request involved,
which is why `FRONTEND_ORIGIN` stays unset in `prod`. `BACKEND_HOST` is declared in `wrangler.toml`'s
`[vars]` as `https://api.umfantasyleague.com` — safe to commit because it names a named Cloudflare
Tunnel (see the `cloudflared` service above), not the old ad-hoc quick tunnel whose rotating
`trycloudflare.com` address had to live dashboard-only (Workers & Pages → the Worker → Settings →
Variables and Secrets) to avoid publishing a hostname that would go stale on the next restart.
`wrangler.toml` still sets `keep_vars = true`, now just belt-and-braces against a *future*
dashboard-only var being wiped — `wrangler deploy` treats `[vars]` as the complete set of plain-text
variables and resets anything dashboard-only to what's in the file on every deploy, which on this
repo means every push to `master` (Cloudflare's Git-connected build runs `wrangler deploy`). Keep
`BACKEND_HOST` in step with `VITE_API_PROXY_TARGET`/the `server.proxy` target in
`frontend/vite.config.ts` (see Commands above) so dev and prod hit the same API — dev's default stays
`http://localhost:8080` since a dev session runs its own backend rather than going through the
tunnel.
