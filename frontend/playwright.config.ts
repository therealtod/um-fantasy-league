import { fileURLToPath, URL } from 'node:url'
import { defineConfig, devices } from '@playwright/test'

const here = fileURLToPath(new URL('.', import.meta.url))

/**
 * Dev-mode auth resolves one identity per running frontend, baked in at
 * Vite startup via `VITE_DEV_MANAGER_ID` (see `src/lib/developmentIdentity.ts`)
 * — there is no in-browser way to switch managers the way a real Supabase
 * login would. So "two managers in two browsers" means two Vite dev servers,
 * each pinned to a different seeded manager id, both proxying `/api` to the
 * same backend. `Player A`/`Player B` in the spec map onto these ports.
 *
 * Neither the backend nor Postgres is started here — `global-setup.ts` just
 * checks the backend is already up (via `docker compose up -d db flyway` +
 * `cd backend-rs && cargo run -p umfl-server`, or `docker compose up -d`, per
 * AGENTS.md) and fails fast with instructions if it isn't. A cold `cargo`
 * build is too slow and stateful to reliably manage as a Playwright
 * `webServer`.
 */
export const PLAYER_A_URL = 'http://localhost:5273'
export const PLAYER_B_URL = 'http://localhost:5274'
export const BACKEND_URL = 'http://localhost:8080'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  reporter: 'list',
  globalSetup: './e2e/global-setup.ts',
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'desktop', testMatch: /tournament-lifecycle\.spec\.ts/, use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile', testMatch: /responsive\.spec\.ts/, use: { ...devices['Pixel 5'] } },
  ],
  webServer: [
    {
      command: 'npm run dev -- --port 5273 --strictPort',
      cwd: here,
      url: PLAYER_A_URL,
      env: { VITE_DEV_MANAGER_ID: '1' }, // NeonStrategist — also the seeded admin.
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
    {
      command: 'npm run dev -- --port 5274 --strictPort',
      cwd: here,
      url: PLAYER_B_URL,
      env: { VITE_DEV_MANAGER_ID: '2' }, // SherlockMain.
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
  ],
})
