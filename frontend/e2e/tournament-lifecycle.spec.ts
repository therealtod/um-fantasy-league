import { test, expect, type APIRequestContext, type Page } from '@playwright/test'
import { BACKEND_URL, PLAYER_A_URL, PLAYER_B_URL } from '../playwright.config'

/**
 * The whole real-world journey, driven through two real, separately
 * authenticated browser sessions: an admin stands up a tournament, two
 * managers register and draft inside budget, the admin locks in live
 * results, and the standings page — rendered by the real Vue app against the
 * real backend — names the winner.
 *
 * This is the browser-level counterpart to
 * `backend/src/test/kotlin/com/umfl/TournamentLifecycleIntegrationTest.kt`
 * and `src/stores/tournamentLifecycle.spec.ts`: those exercise the service
 * layer and the Pinia stores respectively; this one is the only one of the
 * three that actually renders a page and clicks a button.
 *
 * Admin setup (tournament, hero/map pool, scoring, match results) goes
 * through the real Admin API via HTTP, exactly like `AdminDashboardView`'s
 * wizards would — except the Match Results section of that dashboard is
 * still a stub (a hard-coded "Use Demo Tournament (ID: 1)" button, no real
 * tournament picker), so recording results through it isn't yet possible.
 * The two things that ARE real, finished product surfaces — registering,
 * drafting, locking a roster, and reading the standings board — are the
 * parts this test puts an actual browser against.
 */

const ADMIN_MANAGER_ID = 1 // NeonStrategist — also the seeded admin (V4 migration).
const PLAYER_B_MANAGER_ID = 2 // SherlockMain.

const TOURNAMENT_BASE = {
  format: 'ARSENAL',
  startDate: '2026-11-01',
  capacity: 8,
  rosterSize: 3,
  creditGrant: 10_000,
}

async function meAs(managerId: number) {
  const response = await fetch(`${BACKEND_URL}/api/me`, {
    headers: { 'X-Manager-Id': String(managerId) },
  })
  return response.json() as Promise<{ handle: string; isAdmin: boolean }>
}

/** Selects the exact hero card by its name — `HeroCard.vue` renders the whole card as one `<button>`. */
async function pickHero(page: Page, name: string) {
  await page.locator('button').filter({ has: page.getByText(name, { exact: true }) }).click()
}

test.describe.configure({ mode: 'serial' })

test.describe('tournament lifecycle', () => {
  let adminApi: APIRequestContext
  let tournamentId: number
  let tournamentName: string
  let heroIdOf: Record<string, number>
  let mapIdOf: Record<string, number>

  test.beforeAll(async ({ playwright }) => {
    adminApi = await playwright.request.newContext({
      baseURL: BACKEND_URL,
      extraHTTPHeaders: { 'X-Manager-Id': String(ADMIN_MANAGER_ID) },
    })

    const heroes = await (await adminApi.get('/api/admin/heroes')).json() as { id: number; name: string }[]
    const maps = await (await adminApi.get('/api/admin/maps')).json() as { id: number; name: string }[]
    heroIdOf = Object.fromEntries(heroes.map((h) => [h.name, h.id]))
    mapIdOf = Object.fromEntries(maps.map((m) => [m.name, m.id]))
  })

  test.afterAll(async () => {
    if (tournamentId) {
      await adminApi.delete(`/api/admin/tournaments/${tournamentId}`)
    }
    await adminApi.dispose()
  })

  async function setTournamentStatus(status: string, endDate: string | null = null) {
    const response = await adminApi.put(`/api/admin/tournaments/${tournamentId}`, {
      data: { ...TOURNAMENT_BASE, name: tournamentName, status, endDate },
    })
    expect(response.ok(), await response.text()).toBeTruthy()
  }

  async function recordMatch(
    round: number,
    mapName: string,
    winner: { player: string; hero: string; health: number },
    loser: { player: string; hero: string; health: number },
  ) {
    const response = await adminApi.post(`/api/admin/tournaments/${tournamentId}/matches`, {
      data: {
        round,
        mapId: mapIdOf[mapName],
        playedAt: new Date().toISOString(),
        participants: [
          { playerLabel: winner.player, heroId: heroIdOf[winner.hero], healthRemaining: winner.health, isWinner: true },
          { playerLabel: loser.player, heroId: heroIdOf[loser.hero], healthRemaining: loser.health, isWinner: false },
        ],
        bans: [],
      },
    })
    expect(response.ok(), await response.text()).toBeTruthy()
  }

  test('a tournament runs from creation to a decided winner, end to end in real browsers', async ({ browser }) => {
    await test.step('the seed ids this test hardcodes still mean what they used to', async () => {
      const admin = await meAs(ADMIN_MANAGER_ID)
      expect(admin.handle).toBe('NeonStrategist')
      expect(admin.isAdmin).toBe(true)
      expect((await meAs(PLAYER_B_MANAGER_ID)).handle).toBe('SherlockMain')
    })

    await test.step('an admin stands up a tournament: pool, board, scoring', async () => {
      tournamentName = `E2E Championship ${Date.now()}`

      const created = await adminApi.post('/api/admin/tournaments', {
        data: { ...TOURNAMENT_BASE, name: tournamentName, status: 'SCHEDULED', endDate: null },
      })
      expect(created.ok(), await created.text()).toBeTruthy()
      tournamentId = (await created.json()).id

      for (const hero of ['Alice', 'King Arthur', 'Robin Hood', 'Medusa', 'Sherlock Holmes', 'Dracula']) {
        const res = await adminApi.put(`/api/admin/tournaments/${tournamentId}/heroes/${heroIdOf[hero]}`, {
          data: { cost: 1_000 },
        })
        expect(res.ok(), await res.text()).toBeTruthy()
      }

      for (const map of ['Baskerville Manor', 'Sherwood Forest', 'Raptor Paddock']) {
        const res = await adminApi.put(`/api/admin/tournaments/${tournamentId}/maps/${mapIdOf[map]}`)
        expect(res.ok(), await res.text()).toBeTruthy()
      }

      const ruleSet = await adminApi.post(`/api/admin/tournaments/${tournamentId}/scoring-rule-sets`, {
        data: {
          name: 'E2E Standard',
          coefficients: [
            { metric: 'WIN', coefficient: 10, sortOrder: 0 },
            { metric: 'HEALTH_REMAINING', coefficient: 1, sortOrder: 1 },
            { metric: 'APPEARANCE', coefficient: 1, sortOrder: 2 },
          ],
          activate: true,
        },
      })
      expect(ruleSet.ok(), await ruleSet.text()).toBeTruthy()

      await setTournamentStatus('REGISTRATION_OPEN')
    })

    const draftAndLock = async (playerUrl: string, heroes: string[]) => {
      const context = await browser.newContext()
      const page = await context.newPage()
      await page.goto(`${playerUrl}/lobby`)

      const card = page.locator('li').filter({ hasText: tournamentName })
      await expect(card).toBeVisible()
      await card.getByRole('button', { name: 'Register' }).click()
      await page.waitForURL(new RegExp(`/tournaments/${tournamentId}/roster`))

      for (const [index, hero] of heroes.entries()) {
        await expect(page.getByText(hero, { exact: true })).toBeVisible()
        await pickHero(page, hero)
        await expect(
          page.getByRole('button', { name: `Lock Roster (${index + 1}/${heroes.length})` }),
        ).toBeVisible()
      }

      await page.getByRole('button', { name: `Lock Roster (${heroes.length}/${heroes.length})` }).click()
      await expect(page.getByText('Roster Locked', { exact: true })).toBeVisible()

      return context
    }

    let playerAContext: Awaited<ReturnType<typeof browser.newContext>> | undefined
    let playerBContext: Awaited<ReturnType<typeof browser.newContext>> | undefined

    try {
      await test.step('manager A registers and drafts a roster in a real browser', async () => {
        playerAContext = await draftAndLock(PLAYER_A_URL, ['Alice', 'King Arthur', 'Robin Hood'])
      })

      await test.step('manager B registers and drafts a different roster in a second real browser', async () => {
        playerBContext = await draftAndLock(PLAYER_B_URL, ['Medusa', 'Sherlock Holmes', 'Dracula'])
      })

      await test.step('matches go live and the admin records results round by round', async () => {
        await setTournamentStatus('LIVE')

        await recordMatch(
          1,
          'Baskerville Manor',
          { player: 'Tomas Ferreira', hero: 'Alice', health: 8 },
          { player: 'Rina Okafor', hero: 'Medusa', health: 0 },
        )
        await recordMatch(
          1,
          'Sherwood Forest',
          { player: 'Dmitri Kovac', hero: 'King Arthur', health: 10 },
          { player: 'Hana Sato', hero: 'Sherlock Holmes', health: 2 },
        )
        await recordMatch(
          2,
          'Raptor Paddock',
          { player: 'Aurelie Blanc', hero: 'Robin Hood', health: 6 },
          { player: 'Miles Ashworth', hero: 'Dracula', health: 3 },
        )

        await setTournamentStatus('COMPLETED', '2026-11-03')
      })

      await test.step('the standings page names the winner, in a real browser', async () => {
        const page = await playerAContext!.newPage()
        await page.goto(`${PLAYER_A_URL}/standings`)
        await page.selectOption('#standings-tournament', String(tournamentId))

        const winnerRow = page.getByRole('row').filter({ hasText: 'NeonStrategist' })
        await expect(winnerRow).toBeVisible()
        await expect(winnerRow.locator('td').first()).toHaveText('1')
        await expect(winnerRow).toContainText('57.0')
        await expect(winnerRow).toContainText('Alice · King Arthur · Robin Hood')

        const runnerUpRow = page.getByRole('row').filter({ hasText: 'SherlockMain' })
        await expect(runnerUpRow.locator('td').first()).toHaveText('2')
        await expect(runnerUpRow).toContainText('8.0')
      })
    } finally {
      await playerAContext?.close()
      await playerBContext?.close()
    }
  })
})
