/**
 * Shared plumbing for the two Tabletop League scrapers (`scrape.mjs` — the match list —
 * and `scrape-match.mjs` — a single match's detail page). The two pages render very
 * different DOM (the detail page has a Draft Details section, per-game hero cards, etc.
 * that the list page doesn't), so there's little parsing logic worth sharing between
 * them — what they do share is browser lifecycle, output, and URL handling, which live
 * here so both scripts stay small and in step.
 */

import { chromium } from "playwright";
import { writeFile } from "node:fs/promises";

export const SITE_ORIGIN = "https://www.tabletopleague.com";

export const DEFAULT_COMPETITION_MATCHES_URL = `${SITE_ORIGIN}/o/umleague/summer-of-legends-2026/matches`;

/**
 * Minimal `--flag value` / `--flag` (boolean) parser shared by both CLIs.
 *
 * `spec` maps a flag name (without `--`) to `{ hasValue: boolean, default: any }`.
 * Returns the parsed values under camelCase keys (`max-pages` -> `maxPages`), plus any
 * bare positional arguments under `_`.
 */
export function parseArgs(argv, spec) {
  const args = { _: [] };
  for (const [name, def] of Object.entries(spec)) {
    args[toCamel(name)] = def.default;
  }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") {
      args.help = true;
      continue;
    }
    if (!a.startsWith("--")) {
      args._.push(a);
      continue;
    }
    const name = a.slice(2);
    const def = spec[name];
    if (!def) {
      throw new Error(`Unknown argument: ${a}`);
    }
    args[toCamel(name)] = def.hasValue ? argv[++i] : true;
  }
  return args;
}

function toCamel(kebab) {
  return kebab.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
}

/** Resolves a possibly-relative URL (as the list scraper's `detailsUrl` is) against the site origin. */
export function resolveSiteUrl(maybeRelative) {
  return new URL(maybeRelative, SITE_ORIGIN).toString();
}

export function matchIdFromUrl(url) {
  const m = url.match(/\/matches\/([0-9a-f-]{8,})/i);
  return m ? m[1] : null;
}

export async function launchBrowser({ headful = false } = {}) {
  const browser = await chromium.launch({ headless: !headful });
  const page = await browser.newPage();
  return { browser, page };
}

/** Navigate and give the page's entrance animations a moment to settle before reading the DOM. */
export async function politeGoto(page, url) {
  await page.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(500);
}

export async function writeJsonFile(path, data) {
  await writeFile(path, JSON.stringify(data, null, 2), "utf8");
}

/** `await sleep(ms)` — used between requests to stay polite (see README's "Scope and etiquette"). */
export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
