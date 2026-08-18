#!/usr/bin/env node
/**
 * Scrapes a Tabletop League competition's public "Matches" page (e.g. an Unmatched
 * tournament) into JSON.
 *
 * Why this exists: tabletopleague.com renders its match list client-side (Next.js,
 * React Server Components) — the raw HTML has no match data in it, only the JS
 * bundle. A plain `fetch`/`curl` cannot see match results; this uses a real
 * (headless) browser and reads the rendered DOM instead.
 *
 * Scope, deliberately: this only ever navigates the public `/o/<org>/<competition>/matches`
 * page. It never calls the site's own `/api/**` routes or its Supabase backend directly,
 * even though those are technically reachable from a browser — tabletopleague.com's
 * robots.txt disallows `/api/` (see `robots.txt` at the site root), and the match list
 * page itself is the only thing this tool is meant to touch. Keep it that way if you
 * extend this script.
 *
 * Output shape mirrors what the page actually shows, not this repo's Admin API request
 * shape (`POST /api/admin/matches`) — see README.md in this folder for why the two don't
 * line up 1:1. In particular, bans and the pick order aren't on this list page at all —
 * for those, feed this script's output into `scrape-match.mjs --from-list` (see that
 * script and the README).
 *
 * Usage:
 *   npm install                       # once, installs the local `playwright` package
 *   node scrape.mjs --url <matches-page-url> [options]
 *
 * Options:
 *   --url <url>          Competition matches page. Defaults to the Summer of Legends 2026
 *                         UMLeague competition this tool was written against.
 *   --out <path>          Where to write the JSON array (default: matches.json, this folder)
 *   --max-pages <n>        Stop after this many pages (default: all pages, per "Page X of Y")
 *   --delay-ms <n>          Pause between page loads, in ms (default: 1500 — be polite,
 *                         this is someone else's production database)
 *   --headful              Show the browser window (for debugging selector drift)
 *
 * The site's markup is ordinary Tailwind-class React output with no stable data-* hooks
 * beyond `data-testid="match-series-score"` and `data-slot="card"`. If Tabletop League
 * changes its component markup, the CSS-class selectors below will need updating — this
 * is expected maintenance for scraping a third-party site you don't control, not a bug
 * in the approach.
 */

import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import {
  DEFAULT_COMPETITION_MATCHES_URL,
  launchBrowser,
  parseArgs as parseArgsShared,
  politeGoto,
  writeJsonFile,
} from "./lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));

const ARG_SPEC = {
  url: { hasValue: true, default: DEFAULT_COMPETITION_MATCHES_URL },
  out: { hasValue: true, default: null }, // resolved against cwd below, once parsed
  "max-pages": { hasValue: true, default: Infinity },
  "delay-ms": { hasValue: true, default: 1500 },
  headful: { hasValue: false, default: false },
};

function parseArgs(argv) {
  const args = parseArgsShared(argv, ARG_SPEC);
  if (args.help) {
    printHelp();
    process.exit(0);
  }
  args.out = resolve(process.cwd(), args.out ?? resolve(__dirname, "matches.json"));
  args.maxPages = args.maxPages === Infinity ? Infinity : Number(args.maxPages);
  args.delayMs = Number(args.delayMs);
  return args;
}

function printHelp() {
  console.log(`Usage: node scrape.mjs [--url <matches-page-url>] [--out <file.json>]
                   [--max-pages <n>] [--delay-ms <n>] [--headful]

Defaults:
  --url        ${DEFAULT_COMPETITION_MATCHES_URL}
  --out        ./matches.json
  --max-pages  all pages
  --delay-ms   1500`);
}

/**
 * Runs inside the page (browser context) — must be self-contained, no closures over
 * Node-side variables.
 *
 * Extraction strategy: for the per-game hero attribution, we deliberately do NOT trust
 * the "P1"/"P2" tag printed next to each hero in a game row — spot-checking the live
 * page shows that tag flips sides between games within the same series (it tracks
 * something like per-game seating/pick order, not which of the two series competitors
 * it is). What's a stable per-side identifier is the header's comma-separated hero list,
 * which is in game order for that side ("Tomoe Gozen, Little Red Riding Hood, Bruce Lee"
 * = that side's G1, G2, G3 hero). So each game row's two hero names are matched back to
 * side A/side B by comparing text against `heroesA[gameIndex]` / `heroesB[gameIndex]`,
 * falling back to left/right DOM position only on a mirror pick the text match can't
 * disambiguate.
 */
function extractMatchesFromPage() {
  const results = [];
  const skipped = [];

  const cards = Array.from(
    document.querySelectorAll('[data-slot="card"]'),
  ).filter((card) => card.querySelector('[data-testid="match-series-score"]'));

  for (const card of cards) {
    try {
      const detailsHref =
        card.querySelector('a[href*="/matches/"]')?.getAttribute("href") ?? null;
      const matchId = detailsHref ? detailsHref.split("/matches/")[1] : null;

      const cardContent = card.querySelector('[data-slot="card-content"]') ?? card;
      const headerGroup = cardContent.querySelector(
        ":scope > div:first-child > div:first-child",
      );
      const headerSpans = Array.from(headerGroup?.querySelectorAll(":scope > span") ?? []).map(
        (s) => s.textContent.trim(),
      );
      // headerSpans is roughly [competitionName, "•", "<date> · <time>"]
      const playedAtRaw = headerSpans.find((t) => t.includes("·")) ?? null;
      const seriesFormat =
        headerGroup?.querySelector(":scope > div")?.textContent.trim() ?? null; // e.g. "BO3"

      const sideBlocks = card.querySelectorAll(
        ":scope .flex.flex-col.sm\\:flex-row.items-center.gap-2.sm\\:gap-3.justify-center > div.flex.items-center.gap-2",
      );
      const sideAEl = sideBlocks[0] ?? null;
      const sideBEl = card.querySelector(
        ":scope .flex.flex-col.sm\\:flex-row.items-center.gap-2.sm\\:gap-3.justify-center > div.flex.items-center.gap-2.flex-row-reverse",
      );

      function readSide(el) {
        if (!el) return { playerLabel: null, heroesInGameOrder: [] };
        const nameSpan = el.querySelector("span.text-sm.font-semibold");
        const heroesSpan = el.querySelector(
          "span.text-xs.text-muted-foreground.truncate",
        );
        const playerLabel = nameSpan?.textContent.trim() ?? null;
        const heroesInGameOrder = (heroesSpan?.textContent ?? "")
          .split(",")
          .map((h) => h.trim())
          .filter(Boolean);
        return { playerLabel, heroesInGameOrder };
      }

      const sideA = readSide(sideAEl);
      const sideB = readSide(sideBEl);

      const scoreEl = card.querySelector('[data-testid="match-series-score"]');
      // The score element also has a "-" separator span between the two numbers, so
      // filter to numeric text rather than assuming index 0/1 are the two scores.
      const scoreSpans = scoreEl
        ? Array.from(scoreEl.querySelectorAll("span"))
            .map((s) => s.textContent.trim())
            .filter((t) => /^\d+$/.test(t))
        : [];
      const scoreA = scoreSpans[0] !== undefined ? Number(scoreSpans[0]) : null;
      const scoreB = scoreSpans[1] !== undefined ? Number(scoreSpans[1]) : null;
      const seriesWinner =
        Number.isFinite(scoreA) && Number.isFinite(scoreB) && scoreA !== scoreB
          ? scoreA > scoreB
            ? "A"
            : "B"
          : null;

      // Game rows: each starts with a "G<n>" label span.
      const gameLabelSpans = Array.from(card.querySelectorAll("span")).filter(
        (s) => /^G\d+$/.test(s.textContent.trim()),
      );

      const games = gameLabelSpans.map((labelSpan) => {
        const row = labelSpan.closest(
          "div.flex.items-center.gap-2.py-2.px-2.rounded",
        );
        const gameIndex = Number(labelSpan.textContent.trim().slice(1)); // "G2" -> 2
        const mapImg = row?.querySelector(
          "div.relative.h-8.w-12 img[alt]",
        );
        const mapName = mapImg?.getAttribute("alt") ?? null;

        const slotBlocks = row
          ? Array.from(row.querySelectorAll("div.flex-1.min-w-0"))
          : [];

        function readSlot(el) {
          const heroName =
            el.querySelector("span.text-xs.font-medium.truncate")?.textContent.trim() ??
            null;
          const healthText = el.querySelector("span.tabular-nums")?.textContent.trim();
          const health = healthText !== undefined ? Number(healthText) : null;
          const isWinner = !!el.querySelector("svg.lucide-trophy");
          return { heroName, health, isWinner };
        }

        const rawSlots = slotBlocks.map(readSlot);

        // Attribute each slot to series side A/B via that side's expected hero for
        // this game index (1-based), falling back to DOM left/right order.
        const expectedA = sideA.heroesInGameOrder[gameIndex - 1] ?? null;
        const expectedB = sideB.heroesInGameOrder[gameIndex - 1] ?? null;

        let slotA = rawSlots.find((s) => s.heroName && s.heroName === expectedA);
        let slotB = rawSlots.find((s) => s.heroName && s.heroName === expectedB);
        if (!slotA || !slotB || slotA === slotB) {
          slotA = rawSlots[0] ?? null;
          slotB = rawSlots[1] ?? null;
        }

        return {
          gameIndex,
          mapName,
          sideA: slotA
            ? { heroName: slotA.heroName, health: slotA.health, isWinner: slotA.isWinner }
            : null,
          sideB: slotB
            ? { heroName: slotB.heroName, health: slotB.health, isWinner: slotB.isWinner }
            : null,
        };
      });

      results.push({
        matchId,
        detailsUrl: detailsHref,
        playedAtRaw,
        seriesFormat,
        sideA: { playerLabel: sideA.playerLabel, heroesInGameOrder: sideA.heroesInGameOrder },
        sideB: { playerLabel: sideB.playerLabel, heroesInGameOrder: sideB.heroesInGameOrder },
        score: { sideA: scoreA, sideB: scoreB },
        seriesWinner,
        games,
      });
    } catch (err) {
      skipped.push({ reason: String(err), snippet: card.textContent.trim().slice(0, 200) });
    }
  }

  return { results, skipped };
}

async function run() {
  const args = parseArgs(process.argv.slice(2));
  console.log(`Scraping ${args.url}`);
  console.log(`(max-pages=${args.maxPages === Infinity ? "all" : args.maxPages}, delay-ms=${args.delayMs})`);

  const { browser, page } = await launchBrowser({ headful: args.headful });

  const allMatches = [];
  const allSkipped = [];

  try {
    await politeGoto(page, args.url);

    let pageNum = 1;
    while (true) {
      await page.waitForTimeout(500); // let the "animate-in" cards settle
      const { results, skipped } = await page.evaluate(extractMatchesFromPage);
      allMatches.push(...results);
      allSkipped.push(...skipped);
      console.log(`  page ${pageNum}: ${results.length} matches (${skipped.length} skipped)`);

      if (pageNum >= args.maxPages) break;

      const nextButton = page.getByRole("button", { name: "Next page" });
      const isDisabled = await nextButton.isDisabled().catch(() => true);
      if (isDisabled) break;

      await nextButton.click();
      await page.waitForTimeout(args.delayMs);
      pageNum++;
    }
  } finally {
    await browser.close();
  }

  const output = {
    scrapedAt: new Date().toISOString(),
    sourceUrl: args.url,
    matchCount: allMatches.length,
    matches: allMatches,
    skipped: allSkipped,
  };

  await writeJsonFile(args.out, output);
  console.log(`\nWrote ${allMatches.length} matches (${allSkipped.length} skipped) to ${args.out}`);
  if (allSkipped.length > 0) {
    console.log("Some cards could not be parsed — inspect the \"skipped\" array in the output.");
  }
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
