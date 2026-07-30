import { BACKEND_URL } from '../playwright.config'

/**
 * Fails fast, with the exact commands to fix it, instead of letting every
 * test in the run time out one by one against a backend that was never
 * started.
 */
export default async function globalSetup() {
  const attempts = 10
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      const response = await fetch(`${BACKEND_URL}/api/tournaments`)
      if (response.ok) return
    } catch {
      // Connection refused — backend not up yet, or not up at all. Keep trying.
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000))
  }

  throw new Error(
    `Backend not reachable at ${BACKEND_URL} after ${attempts}s. Start it first:\n` +
      '  docker compose up -d db\n' +
      "  ./gradlew :backend:bootRun --args='--spring.profiles.active=dev'\n" +
      'or: docker compose up -d',
  )
}
