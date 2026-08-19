#!/usr/bin/env node
/**
 * HTTP wrapper around `scrape-match.mjs`'s single-match scrape, so this repo's
 * backend can import a match by URL instead of an operator running the CLI and
 * pasting JSON.
 *
 * It exists as a separate Node process for one reason: tabletopleague.com is
 * client-rendered Next.js, so reading a match needs a real browser (see the
 * README's "Why a headless browser, not curl"). The Kotlin backend has no
 * headless browser and its runtime image is an Alpine JRE -- the wrong base for
 * Playwright -- so the browser lives here and the backend talks to it over HTTP.
 *
 * Docker is only how this reaches the VPS; it is a plain Node process and runs
 * the same way without a container:
 *
 *   npm install && npx playwright install chromium   # once
 *   npm run serve
 *
 * Routes:
 *   GET  /health         -> {"status":"UP"}
 *   POST /scrape/match   {"url": "..."} -> the scrapeOne() result verbatim
 *
 * Environment:
 *   SCRAPER_HOST   bind address (default 127.0.0.1 -- loopback, not the world)
 *   SCRAPER_PORT   port (default 3000)
 *   SCRAPER_DELAY_MS  politeness delay enforced between scrapes (default 1500)
 */

import { createServer } from "node:http";
import { chromium } from "playwright";
import { SITE_ORIGIN, sleep } from "./lib.mjs";
import { scrapeOne } from "./scrape-match.mjs";

const HOST = process.env.SCRAPER_HOST ?? "127.0.0.1";
const PORT = Number(process.env.SCRAPER_PORT ?? 3000);
const DELAY_MS = Number(process.env.SCRAPER_DELAY_MS ?? 1500);

/** Largest request body we'll buffer. A `{"url": "..."}` object is a few hundred bytes. */
const MAX_BODY_BYTES = 8 * 1024;

const ALLOWED_HOST = new URL(SITE_ORIGIN).host;

/**
 * Only public match *detail* pages are fetchable through this server.
 *
 * The CLI's etiquette (README, "Scope and etiquette") is that these tools never
 * touch the site's own `/api/**` routes or anything robots.txt disallows. A
 * server taking a URL from a caller could trivially become a general-purpose
 * fetcher for anything the container can reach, so that etiquette is enforced
 * here as a check rather than left as a convention -- host must be the site,
 * path must be `/o/<org>/<competition>/matches/<id>`.
 */
function validateMatchUrl(raw) {
  let url;
  try {
    url = new URL(raw);
  } catch {
    return { error: "url is not a valid absolute URL" };
  }
  if (url.protocol !== "https:") {
    return { error: "url must be https" };
  }
  if (url.host !== ALLOWED_HOST) {
    return { error: `url host must be ${ALLOWED_HOST}` };
  }
  if (!/^\/o\/[^/]+\/[^/]+\/matches\/[0-9a-f-]{8,}\/?$/i.test(url.pathname)) {
    return { error: "url must be a match detail page: /o/<org>/<competition>/matches/<id>" };
  }
  return { url: url.toString() };
}

// --- Browser lifecycle -----------------------------------------------------
// One browser for the process lifetime rather than one per request: launching
// Chromium costs far more than the scrape itself. Launched lazily so the
// container is healthy (and cheap) before the first import.

let browserPromise = null;

async function getBrowser() {
  if (browserPromise) {
    const browser = await browserPromise;
    if (browser.isConnected()) return browser;
    // Chromium died (OOM-killed, crashed). Drop it and launch a fresh one
    // rather than handing back a dead handle for every later request.
    browserPromise = null;
  }
  browserPromise = chromium.launch({ headless: true });
  return browserPromise;
}

// --- Request queue ---------------------------------------------------------
// Scrapes run strictly one at a time with the CLI's politeness delay between
// them. This is someone else's production database (README again), and
// concurrent tabs would also multiply this container's memory footprint.

let queue = Promise.resolve();
let lastScrapeEndedAt = 0;

function enqueue(task) {
  const run = queue.then(task, task);
  // Keep the chain alive after a rejection: the caller owns this error.
  queue = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

async function scrapeQueued(url) {
  return enqueue(async () => {
    const sinceLast = Date.now() - lastScrapeEndedAt;
    if (lastScrapeEndedAt > 0 && sinceLast < DELAY_MS) {
      await sleep(DELAY_MS - sinceLast);
    }
    const browser = await getBrowser();
    const page = await browser.newPage();
    try {
      return await scrapeOne(page, url);
    } finally {
      await page.close().catch(() => {});
      lastScrapeEndedAt = Date.now();
    }
  });
}

// --- HTTP ------------------------------------------------------------------

function sendJson(res, status, body) {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(payload),
  });
  res.end(payload);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on("data", (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error("request body too large"));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

const server = createServer(async (req, res) => {
  const path = (req.url ?? "/").split("?")[0];

  if (req.method === "GET" && path === "/health") {
    sendJson(res, 200, { status: "UP" });
    return;
  }

  if (req.method !== "POST" || path !== "/scrape/match") {
    sendJson(res, 404, { error: "not found" });
    return;
  }

  let payload;
  try {
    payload = JSON.parse(await readBody(req));
  } catch (err) {
    sendJson(res, 400, { error: `invalid JSON body: ${err.message}` });
    return;
  }

  const { url, error } = validateMatchUrl(payload?.url ?? "");
  if (error) {
    sendJson(res, 400, { error });
    return;
  }

  try {
    console.log(`scraping ${url}`);
    const result = await scrapeQueued(url);
    sendJson(res, 200, result);
  } catch (err) {
    // A scrape failure is almost always selector drift or a page that never
    // settled -- surface the message so the backend can name it to the admin
    // rather than reporting a bare 500.
    console.error(`scrape failed for ${url}:`, err);
    sendJson(res, 502, { error: `scrape failed: ${err.message}` });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`tabletopleague scraper listening on http://${HOST}:${PORT}`);
});

async function shutdown(signal) {
  console.log(`\n${signal} received, shutting down`);
  server.close();
  if (browserPromise) {
    await (await browserPromise).close().catch(() => {});
  }
  process.exit(0);
}

process.on("SIGTERM", () => void shutdown("SIGTERM"));
process.on("SIGINT", () => void shutdown("SIGINT"));
