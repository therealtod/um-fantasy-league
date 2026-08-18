# Tabletop League scraper

Read-only Playwright scripts that scrape [tabletopleague.com](https://www.tabletopleague.com)
competition pages into JSON — e.g. a real Unmatched tournament running on that platform, whose
results you might want to hand-enter through this repo's `/api/admin/matches`.

Two scripts, one shared library:

| Script | Scrapes | Gets you |
|---|---|---|
| `scrape.mjs` | A competition's `/matches` list page, all pages | Every match's score, per-game hero/map/health, at whatever volume the page allows |
| `scrape-match.mjs` | One match's `/matches/<uuid>` detail page | Everything the list has, **plus** the round/group name, draft format, timezone-qualified timestamp, and the full draft — every pick in order and every ban, typed `PRE_BAN`/`OPPONENT_BAN`/`SELF_BAN` |
| `lib.mjs` | — | Browser lifecycle, CLI arg parsing, and URL helpers shared by both |

This is a standalone tool, not part of the Gradle build or the `frontend/` npm project — it has its
own `package.json` and is meant to be run manually, on demand.

## Why a headless browser, not `curl`

Both pages are client-rendered Next.js: the raw HTML has no match data in it, only the JS bundle. A
plain HTTP fetch gets you nothing. These scripts use Playwright (Chromium) to actually render the
page and read the DOM once React has populated it.

## Why two scripts instead of one

The list page and the detail page render genuinely different markup for genuinely different data —
the detail page has a whole "Draft Details" card (picks and bans) that doesn't exist on the list
page at all, and even the fields both pages share (map, hero, health) sit in differently-shaped
cards. Forcing one extractor to understand both shapes would just make each page's selectors more
fragile for no reuse benefit. What *is* shared — launching/closing the browser, parsing `--flag`
arguments, resolving a relative `detailsUrl` to a real URL, writing JSON — lives in `lib.mjs`, which
both scripts import.

### Linking them: list → detail

`scrape.mjs`'s output already carries each match's `detailsUrl`. Feed that straight to
`scrape-match.mjs --from-list` to fetch the full draft for every match a list scrape found:

```bash
node scrape.mjs --out matches.json
node scrape-match.mjs --from-list matches.json --out matches-detailed.json
```

This is the "reuse" path in practice: run the cheap list scrape to find *which* matches exist, then
selectively (or fully) enrich them with the expensive per-match detail fetch — one request per match
either way, so `--from-list` is exactly as many requests as matches in the file. Point `--out` at a
smaller hand-trimmed copy of `matches.json` if you only want a handful of matches' full detail rather
than the whole competition's.

## Scope and etiquette

Both scripts only ever navigate public `/o/<org>/<competition>/matches...` pages — never the site's
own `/api/**` routes or its Supabase backend directly, even though a browser session can technically
see both. `tabletopleague.com/robots.txt` disallows `/api/` (except `/api/og/`); these tools stay
inside what that file allows. They also:

- run requests sequentially, one browser tab, with a polite delay between them (`--delay-ms`,
  default 1500ms, in both list pagination and `--from-list` batches) — this is someone else's
  production database, not a load target;
- do no polling loop of their own — run them by hand when you want a fresh snapshot;
- never touch `/admin`, `/settings`, `/account`, or anything else `robots.txt` disallows.

## Usage

```bash
npm install                 # once — installs the local Playwright package
```

### `scrape.mjs` — the match list

```bash
node scrape.mjs              # scrapes every page, writes ./matches.json
```

| Flag | Default | Meaning |
|---|---|---|
| `--url <url>` | the Summer of Legends 2026 UMLeague competition | Any competition's `/matches` page |
| `--out <path>` | `./matches.json` | Where to write the JSON |
| `--max-pages <n>` | all pages (per the site's own "Page X of Y") | Cap for a quick/partial run |
| `--delay-ms <n>` | `1500` | Pause between page loads |
| `--headful` | off | Show the browser window — useful if selectors stop matching and you want to look |

### `scrape-match.mjs` — a single match's full detail

Three ways to point it at match(es):

```bash
node scrape-match.mjs --url "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-bf1b-4be1-93ad-f1bdffbdbc2e"
node scrape-match.mjs --id 92569156-bf1b-4be1-93ad-f1bdffbdbc2e            # resolved against --base
node scrape-match.mjs --from-list matches.json --out matches-detailed.json # every match in a list scrape
```

| Flag | Default | Meaning |
|---|---|---|
| `--url <url>` | — | A single match's detail page |
| `--id <uuid>` | — | A single match id, combined with `--base` |
| `--base <url>` | the Summer of Legends 2026 UMLeague competition | Competition the `--id` is resolved against |
| `--from-list <path>` | — | A `scrape.mjs` output file — every match's `detailsUrl` is fetched |
| `--out <path>` | stdout for a single match, `match-details.json` for `--from-list` | Where to write the JSON |
| `--delay-ms <n>` | `1500` | Pause between requests in `--from-list` mode |
| `--headful` | off | Show the browser window |

A bare `--url`/`--id` run with no `--out` prints the single match's JSON to stdout, so it composes
with `jq` etc.

Playwright's Chromium build is downloaded into `node_modules` on `npm install`; if that ever fails in
a sandboxed environment, run `npx playwright install chromium` once.

## Output shape

### `scrape.mjs`

One JSON object with a top-level `matches` array. Each entry:

```jsonc
{
  "matchId": "92569156-bf1b-4be1-93ad-f1bdffbdbc2e",   // the site's own match UUID
  "detailsUrl": "/o/umleague/summer-of-legends-2026/matches/92569156-...",
  "playedAtRaw": "Aug 17, 2026 · 10:00 PM",             // no timezone on the list page — see
                                                          // scrape-match.mjs for a tz-qualified one
  "seriesFormat": "BO3",
  "sideA": { "playerLabel": "mystic_owl", "heroesInGameOrder": ["Tomoe Gozen", "..."] },
  "sideB": { "playerLabel": "immortal",   "heroesInGameOrder": ["Wyatt Earp", "..."] },
  "score": { "sideA": 1, "sideB": 2 },
  "seriesWinner": "B",                                   // "A" | "B" | null (tie/unparseable)
  "games": [
    {
      "gameIndex": 1,
      "mapName": "Technodrome",
      "sideA": { "heroName": "Tomoe Gozen", "health": 5, "isWinner": true },
      "sideB": { "heroName": "Wyatt Earp",  "health": 0, "isWinner": false }
    }
    // ...
  ]
}
```

Any card the extractor couldn't parse lands in the top-level `skipped` array with a text snippet,
rather than corrupting or silently dropping a match — check that array after a run.

**Note on `sideA`/`sideB` vs. the "P1"/"P2" labels on the page**: the rendered page tags each game's
two heroes "P1"/"P2", but that tag flips which series side it points at from game to game within the
same match (it tracks something like per-game seating or pick order, not a stable competitor
identity). This script ignores that tag and instead matches each game's hero name back to the side
whose header hero list (which *is* in game order) expected it — falling back to left/right DOM
position only on an exact mirror pick the name match can't disambiguate.

### `scrape-match.mjs`

One JSON object per match:

```jsonc
{
  "matchId": "92569156-bf1b-4be1-93ad-f1bdffbdbc2e",
  "sourceUrl": "https://www.tabletopleague.com/o/umleague/summer-of-legends-2026/matches/92569156-...",
  "title": "mystic_owl vs immortal",
  "status": "Completed",
  "competitionName": "Summer of Legends 2026",
  "competitionUrl": "/o/umleague/summer-of-legends-2026",
  "roundName": "The Wayward Sisters",         // this org names its pools after Unmatched heroes/sets —
                                                // "Muhammad Ali" and "Robert Muldoon" show up as real
                                                // round names elsewhere, not a parsing bug
  "seriesFormat": "BO3",
  "draftType": "arsenal draft (3/5/2)",
  "playedAtRaw": "17 Aug 2026, 22:00 CEST",     // timezone-qualified, unlike the list page
  "headToHeadRaw": "H2H: immortal leads 1-0",
  "sideA": {
    "playerLabel": "mystic_owl",
    "seedLabel": "Follower",                    // this org's seeding/standing badge, not part of this
                                                  // repo's domain model — carried through as-is
    "score": 1,
    "picks": ["Tomoe Gozen", "Little Red Riding Hood", "Bruce Lee"],   // in game order (#1/#2/#3 on the page)
    "bans": [
      { "heroName": "Alice", "banType": "OPPONENT_BAN" },
      { "heroName": "Daredevil", "banType": "SELF_BAN" }
    ]
  },
  "sideB": { /* same shape */ },
  "preBans": ["Raptors", "Achilles", "Eredin", "Yennenga", "Annie Christmas", "Golden Bat"], // PRE_BAN,
                                                  // struck before sides were assigned — belongs to
                                                  // neither side, same as this repo's hero_ban table
  "games": [
    {
      "gameIndex": 1,
      "mapName": "Technodrome",
      "upset": true,               // this org's own "underdog won" marker — not part of this repo's model
      "sideA": { "heroName": "Tomoe Gozen", "playerLabel": "mystic_owl", "health": 5, "isWinner": true, "hasAdvantage": false },
      "sideB": { "heroName": "Wyatt Earp",  "playerLabel": "immortal",  "health": 0, "isWinner": false, "hasAdvantage": true }
    }
    // ...
  ]
}
```

Unlike the list page, every hero block on the detail page is directly labelled with the account name
that piloted it — so `sideA`/`sideB` attribution here doesn't need the list scraper's "match hero
name against game-order position" heuristic; it just matches the label text directly, with a
left/right DOM-position fallback if a label ever fails to match either side exactly.

## This is not a ready-made `/api/admin/matches` request body

`TournamentMatch` (`backend/src/main/kotlin/com/umfl/match/TournamentMatch.kt`) needs a
`tournamentId`, a `round` (an `Int`, not this org's named pools), and hero and map **ids** — this
scraper only ever has names. `scrape-match.mjs` now covers the picks/bans data that the list page
was missing entirely, closing the gap called out in this repo's "draft is recorded in full"
invariant — but it still can't invent the ids your database assigns to those hero/map names, or
know which of your tournaments and which numbered round a scraped match belongs to. Treat this
JSON as raw source material for whatever maps site names onto this league's `hero`/`game_map` rows,
not as a drop-in request body.
