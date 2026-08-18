#!/usr/bin/env node
/**
 * Scrapes a single Tabletop League match's detail page into JSON — the richer sibling
 * of `scrape.mjs` (the match list). The detail page (`/o/<org>/<competition>/matches/<uuid>`)
 * carries several things the list page doesn't: the round/group name, the draft format,
 * a timezone-qualified timestamp, and — the reason this script exists — the full draft:
 * every pick in order and every ban, tagged `PRE_BAN` / `OPPONENT_BAN` / `SELF_BAN`
 * exactly like this repo's `HeroBan`/`HeroPick` (`backend/src/main/kotlin/com/umfl/match/TournamentMatch.kt`).
 * The list page shows none of that.
 *
 * Shares browser lifecycle, arg parsing, and URL helpers with `scrape.mjs` via `lib.mjs`
 * — see that file for why the two scripts don't share DOM-parsing code: the two pages'
 * markup differs enough that forcing one extractor to cover both would just make each
 * page's selectors more fragile.
 *
 * Usage — three ways to point it at match(es):
 *
 *   node scrape-match.mjs --url <match-detail-url>
 *   node scrape-match.mjs --id <match-uuid>                    # uses --base's competition
 *   node scrape-match.mjs --from-list matches.json              # every match scrape.mjs found
 *
 * The third form is the "link the two scripts" path: run `scrape.mjs` first to get every
 * match's id for a competition, then hand its output straight to this script to fetch
 * the picks/bans/round-name detail for each one:
 *
 *   node scrape.mjs --out matches.json
 *   node scrape-match.mjs --from-list matches.json --out matches-detailed.json
 *
 * Options:
 *   --url <url>          A single match's detail page
 *   --id <uuid>            A single match id, combined with --base
 *   --base <url>           Competition matches page the id is resolved against
 *                         (default: the Summer of Legends 2026 UMLeague competition)
 *   --from-list <path>      A JSON file from scrape.mjs — every match's `detailsUrl` is fetched
 *   --out <path>           Where to write the JSON (default: stdout for a single match,
 *                         match-details.json for --from-list)
 *   --delay-ms <n>          Pause between requests in --from-list mode (default: 1500)
 *   --headful              Show the browser window (for debugging selector drift)
 */

import { resolve } from "node:path";
import {
  DEFAULT_COMPETITION_MATCHES_URL,
  launchBrowser,
  matchIdFromUrl,
  parseArgs as parseArgsShared,
  politeGoto,
  resolveSiteUrl,
  sleep,
  writeJsonFile,
} from "./lib.mjs";
import { readFile } from "node:fs/promises";

const ARG_SPEC = {
  url: { hasValue: true, default: null },
  id: { hasValue: true, default: null },
  base: { hasValue: true, default: DEFAULT_COMPETITION_MATCHES_URL },
  "from-list": { hasValue: true, default: null },
  out: { hasValue: true, default: null },
  "delay-ms": { hasValue: true, default: 1500 },
  headful: { hasValue: false, default: false },
};

function printHelp() {
  console.log(`Usage:
  node scrape-match.mjs --url <match-detail-url>
  node scrape-match.mjs --id <match-uuid> [--base <competition-matches-url>]
  node scrape-match.mjs --from-list <matches.json> [--out <file.json>] [--delay-ms <n>]

Defaults:
  --base       ${DEFAULT_COMPETITION_MATCHES_URL}
  --delay-ms   1500`);
}

/**
 * Runs inside the page. Self-contained (no closures over Node-side variables) — see
 * `scrape.mjs`'s extractor for the same constraint.
 *
 * Unlike the list page, every hero block on the detail page is directly labelled with
 * the account name that piloted it (`mystic_owl`, `immortal`, ...) — so, unlike the list
 * scraper, there's no need to infer a side from hero-list position; we just match that
 * label text against the two side names read off the summary card.
 */
function extractMatchDetail() {
  const cards = Array.from(document.querySelectorAll('[data-slot="card"]'));

  // --- Header: title, status, competition/round/format/draft-type/date breadcrumb ---
  const h1 = document.querySelector("h1.font-display");
  const title = h1?.textContent.trim() ?? null;
  const headerWrap = h1?.closest(".min-w-0.w-full") ?? null;
  const status =
    headerWrap?.querySelector("span[title]")?.getAttribute("title") ?? null;

  const breadcrumb = headerWrap?.querySelector(
    ":scope > div.flex.flex-wrap.items-center.gap-x-2.gap-y-1",
  );
  const competitionLink = breadcrumb?.querySelector("a");
  const competitionName = competitionLink?.textContent.trim() ?? null;
  const competitionUrl = competitionLink?.getAttribute("href") ?? null;

  const breadcrumbParts = breadcrumb
    ? Array.from(breadcrumb.children)
        .filter((el) => el.tagName !== "A")
        // The date's wrapper span nests its own leading "•" separator + a trophy icon
        // ahead of the date text, so strip any leading bullet/whitespace rather than
        // just filtering out standalone "•" children.
        .map((el) => el.textContent.trim().replace(/^[•\s]+/, ""))
        .filter(Boolean)
    : [];
  let seriesFormat = null;
  let draftType = null;
  let playedAtRaw = null;
  const leftoverParts = [];
  for (const part of breadcrumbParts) {
    if (/^BO\d+$/i.test(part)) seriesFormat = part;
    else if (/draft/i.test(part)) draftType = part;
    else leftoverParts.push(part);
  }
  // Whatever's left: the date (always last, has a day-month-year shape) and, optionally,
  // a round/group name ahead of it.
  if (leftoverParts.length > 0) {
    playedAtRaw = leftoverParts[leftoverParts.length - 1];
    if (leftoverParts.length > 1) {
      // roundName is everything before the date, joined back in case it itself had "•" in it
      var roundName = leftoverParts.slice(0, -1).join(" ");
    }
  }

  // --- Summary card: player names, seed labels, score, H2H ---
  const summaryCard = cards[0] ?? null;
  const desktopSummary = summaryCard?.querySelector(
    '[data-slot="card-content"].p-8, [data-slot="card-content"].hidden.sm\\:block',
  );
  const sideColumns = desktopSummary
    ? Array.from(desktopSummary.querySelectorAll(":scope > div.flex > div"))
    : [];
  // Expect [sideA column, score column, sideB column]; be defensive if that shape drifts.
  const [sideACol, scoreCol, sideBCol] = sideColumns;

  function readSummarySide(col) {
    if (!col) return { playerLabel: null, seedLabel: null };
    const playerLabel =
      col.querySelector(".font-bold.text-xl, .font-bold.text-sm")?.textContent.trim() ?? null;
    const seedLabel =
      col.querySelector(".rounded-full.border")?.textContent.trim() ?? null;
    return { playerLabel, seedLabel };
  }
  const sideASummary = readSummarySide(sideACol);
  const sideBSummary = readSummarySide(sideBCol);

  const scoreNumbers = scoreCol
    ? Array.from(scoreCol.querySelectorAll("span"))
        .map((s) => s.textContent.trim())
        .filter((t) => /^\d+$/.test(t))
    : [];
  const scoreA = scoreNumbers[0] !== undefined ? Number(scoreNumbers[0]) : null;
  const scoreB = scoreNumbers[1] !== undefined ? Number(scoreNumbers[1]) : null;
  const headToHeadRaw =
    scoreCol?.querySelector("span.hidden.sm\\:inline")?.textContent.trim() ?? null;

  // --- Per-game cards: any top-level card that mentions "HP:" is a game card, regardless
  // of its position among the summary/draft-details cards. ---
  const gameCards = cards.filter((c) => /HP:\s*-?\d+/.test(c.textContent));

  function heroBlockFrom(desktopRow, side /* "left" | "right" */) {
    const cols = Array.from(desktopRow.querySelectorAll(":scope > div"));
    const block = side === "left" ? cols[0] : cols[2];
    if (!block) return null;
    const heroName = block.querySelector("span[title]")?.textContent.trim() ?? null;
    const playerLabel =
      block.querySelector("span.text-xs.text-muted-foreground.uppercase, span.text-\\[11px\\].text-muted-foreground.uppercase")
        ?.textContent.trim() ?? null;
    const hpMatch = block.textContent.match(/HP:\s*(-?\d+)/);
    const health = hpMatch ? Number(hpMatch[1]) : null;
    const isWinner = !!block.querySelector("svg.lucide-trophy");
    const hasAdvantage = Array.from(block.querySelectorAll("span")).some(
      (s) => s.textContent.trim() === "Adv",
    );
    return { heroName, playerLabel, health, isWinner, hasAdvantage };
  }

  const games = gameCards.map((card) => {
    const desktopRow = card.querySelector(":scope > div.hidden.sm\\:flex") ?? card;
    const mapBlock = Array.from(desktopRow.querySelectorAll(":scope > div"))[1] ?? null;
    const mapSpans = mapBlock
      ? Array.from(mapBlock.querySelectorAll("span")).map((s) => s.textContent.trim())
      : [];
    const gameLabel = mapSpans.find((t) => /^Game\s+\d+$/i.test(t)) ?? null;
    const gameIndex = gameLabel ? Number(gameLabel.match(/\d+/)[0]) : null;
    const mapName =
      mapSpans.find((t) => t && t !== gameLabel && !/^(Upset)$/i.test(t)) ?? null;
    const upset = /Upset/i.test(mapBlock?.textContent ?? "");

    const left = heroBlockFrom(desktopRow, "left");
    const right = heroBlockFrom(desktopRow, "right");

    // Attribute left/right to sideA/sideB by the account-name label (directly present on
    // this page, unlike the list page — see the extractor's doc comment above).
    let sideAGame = null;
    let sideBGame = null;
    for (const block of [left, right]) {
      if (!block) continue;
      if (block.playerLabel === sideASummary.playerLabel) sideAGame = block;
      else if (block.playerLabel === sideBSummary.playerLabel) sideBGame = block;
    }
    if (!sideAGame || !sideBGame) {
      // Fallback: trust DOM position if label text didn't match cleanly.
      sideAGame = sideAGame ?? left;
      sideBGame = sideBGame ?? right;
    }

    return { gameIndex, mapName, upset, sideA: sideAGame, sideB: sideBGame };
  });
  games.sort((a, b) => (a.gameIndex ?? 0) - (b.gameIndex ?? 0));

  // --- Draft Details card: picks (ordered) and bans (typed) per side, plus the shared
  // pre-ban pool. ---
  const draftCard = cards.find(
    (c) => c.querySelector('[data-slot="card-title"]')?.textContent.trim() === "Draft Details",
  );
  const draftGroups = draftCard
    ? Array.from(
        draftCard.querySelectorAll('[data-slot="card-content"] > div'),
      )
        .filter((div) => div.querySelector("h4"))
        .map((div) => {
          const label = div.querySelector("h4")?.textContent.trim() ?? null;
          const chips = Array.from(div.querySelectorAll(".flex.flex-wrap.gap-2 > div"));
          const picks = [];
          const bans = [];
          for (const chip of chips) {
            const nameSpan = chip.querySelector("span.font-medium");
            const heroName = nameSpan?.textContent.trim() ?? null;
            const trailerSpan = Array.from(chip.querySelectorAll("span")).find(
              (s) => s !== nameSpan,
            );
            const trailer = trailerSpan?.textContent.trim() ?? null;
            if (trailer && /^#\d+$/.test(trailer)) {
              picks.push({ heroName, order: Number(trailer.slice(1)) });
            } else if (trailer === "Opp. ban") {
              bans.push({ heroName, banType: "OPPONENT_BAN" });
            } else if (trailer === "Self ban") {
              bans.push({ heroName, banType: "SELF_BAN" });
            } else {
              // No trailer at all: only the shared "Banned" pool renders chips this way
              // (a hero struck before sides were assigned — PRE_BAN).
              bans.push({ heroName, banType: "PRE_BAN" });
            }
          }
          return { label, picks, bans };
        })
    : [];

  const preBanGroup = draftGroups.find((g) => g.label === "Banned");
  const preBans = preBanGroup ? preBanGroup.bans.map((b) => b.heroName) : [];

  function draftFor(summarySide) {
    const group = draftGroups.find((g) => g.label === summarySide.playerLabel);
    if (!group) return { picks: [], bans: [] };
    return {
      picks: group.picks.sort((a, b) => a.order - b.order).map((p) => p.heroName),
      bans: group.bans,
    };
  }

  return {
    title,
    status,
    competitionName,
    competitionUrl,
    roundName: typeof roundName === "string" ? roundName : null,
    seriesFormat,
    draftType,
    playedAtRaw,
    headToHeadRaw,
    sideA: {
      playerLabel: sideASummary.playerLabel,
      seedLabel: sideASummary.seedLabel,
      score: scoreA,
      ...draftFor(sideASummary),
    },
    sideB: {
      playerLabel: sideBSummary.playerLabel,
      seedLabel: sideBSummary.seedLabel,
      score: scoreB,
      ...draftFor(sideBSummary),
    },
    preBans,
    games: games.map(({ gameIndex, mapName, upset, sideA, sideB }) => ({
      gameIndex,
      mapName,
      upset,
      sideA,
      sideB,
    })),
  };
}

async function scrapeOne(page, url) {
  await politeGoto(page, url);
  const data = await page.evaluate(extractMatchDetail);
  return { matchId: matchIdFromUrl(url), sourceUrl: url, ...data };
}

async function run() {
  const args = parseArgsShared(process.argv.slice(2), ARG_SPEC);
  if (args.help) {
    printHelp();
    process.exit(0);
  }

  const singleUrl = args.url ?? (args.id ? `${args.base.replace(/\/$/, "")}/${args.id}` : null);

  if (!singleUrl && !args.fromList) {
    console.error("Provide one of --url, --id, or --from-list.\n");
    printHelp();
    process.exit(1);
  }

  const { browser, page } = await launchBrowser({ headful: args.headful });
  try {
    if (args.fromList) {
      const listPath = resolve(process.cwd(), args.fromList);
      const list = JSON.parse(await readFile(listPath, "utf8"));
      const entries = list.matches ?? list; // accept either a scrape.mjs output object or a bare array
      const urls = entries
        .map((m) => m.detailsUrl ?? m.sourceUrl ?? m.url)
        .filter(Boolean)
        .map(resolveSiteUrl);

      console.log(`Fetching detail for ${urls.length} matches from ${listPath}`);
      const results = [];
      for (const [i, url] of urls.entries()) {
        console.log(`  [${i + 1}/${urls.length}] ${url}`);
        results.push(await scrapeOne(page, url));
        if (i < urls.length - 1) await sleep(Number(args.delayMs));
      }

      const outPath = resolve(process.cwd(), args.out ?? "match-details.json");
      await writeJsonFile(outPath, {
        scrapedAt: new Date().toISOString(),
        sourceList: listPath,
        matchCount: results.length,
        matches: results,
      });
      console.log(`\nWrote ${results.length} match details to ${outPath}`);
    } else {
      const result = await scrapeOne(page, singleUrl);
      if (args.out) {
        const outPath = resolve(process.cwd(), args.out);
        await writeJsonFile(outPath, result);
        console.log(`Wrote match detail to ${outPath}`);
      } else {
        console.log(JSON.stringify(result, null, 2));
      }
    }
  } finally {
    await browser.close();
  }
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
