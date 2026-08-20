import { test, expect } from '@playwright/test'
import { PLAYER_A_URL } from '../playwright.config'

/**
 * Guards against the specific failure mode a mobile pass can introduce:
 * nothing sets `overflow-x` on `body`, so any element wider than the
 * viewport slides the whole document sideways. This spec runs under the
 * `mobile` Playwright project (Pixel 5 viewport) and, like the rest of the
 * e2e suite, needs a live backend — see `global-setup.ts`.
 */

const ROUTES = ['/lobby', '/standings', '/admin', '/login']

test.describe('responsive layout', () => {
  test('no route scrolls horizontally on a phone viewport', async ({ page }) => {
    for (const route of ROUTES) {
      await test.step(route, async () => {
        await page.goto(`${PLAYER_A_URL}${route}`)
        await page.waitForLoadState('load')
        // Idleness is what we want — an overflow comes from rendered data — but
        // /standings holds an SSE connection open for live results, so the
        // network never goes idle there and an unbounded wait times out the test
        // before it measures anything. Give it a bounded chance, then measure
        // either way: by the time this expires the view has long since painted.
        await page.waitForLoadState('networkidle', { timeout: 5_000 }).catch(() => {})

        const overflow = await page.evaluate(() => ({
          scrollWidth: document.documentElement.scrollWidth,
          innerWidth: window.innerWidth,
        }))
        expect(overflow.scrollWidth, `${route} overflows horizontally`).toBeLessThanOrEqual(overflow.innerWidth + 1)
      })
    }
  })

  test('the nav drawer opens and closes on a phone viewport', async ({ page }) => {
    await page.goto(`${PLAYER_A_URL}/lobby`)

    const standingsLink = page.locator('#app-nav').getByRole('link', { name: 'Standings' })
    await expect(standingsLink).not.toBeVisible()

    await page.getByRole('button', { name: 'Open navigation' }).click()
    await expect(standingsLink).toBeVisible()

    await standingsLink.click()
    await expect(page).toHaveURL(/\/standings/)
    await expect(standingsLink).not.toBeVisible()
  })
})
