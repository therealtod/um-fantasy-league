/**
 * Local-only identity for exercising the dev backend without an OAuth provider.
 * Vite replaces import.meta.env.DEV at build time, so production bundles can
 * never enable this mode by setting an environment variable at runtime.
 */
export function configuredDevelopmentManagerId(
  isDevelopmentBuild: boolean,
  configuredId: string | undefined,
): number | null {
  if (!isDevelopmentBuild || !configuredId) return null

  const managerId = Number(configuredId)
  if (!Number.isSafeInteger(managerId) || managerId < 1) {
    throw new Error('VITE_DEV_MANAGER_ID must be a positive integer')
  }
  return managerId
}

export const developmentManagerId = configuredDevelopmentManagerId(
  import.meta.env.DEV,
  import.meta.env.VITE_DEV_MANAGER_ID,
)

export const isDevelopmentIdentityMode = developmentManagerId !== null
