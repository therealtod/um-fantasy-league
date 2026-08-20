/**
 * `redirect` travels as a route query param (see router guard, api/client's
 * 401 handler, AppShell's sign-in link, and the lobby's register prompt), so
 * it is attacker-controllable input, not a value we minted ourselves. A path
 * is safe to send someone back to only if it can't leave this origin — so a
 * lone leading slash is required (not `//host/...` or `/\host/...`, both of
 * which browsers treat as protocol-relative) and it can't spell out a
 * `scheme:` of its own.
 */
export function isSafeRedirectPath(path: string): boolean {
  if (!path.startsWith('/') || path.startsWith('//') || path.startsWith('/\\')) return false
  return !/^\/[^/\\]*:/.test(path)
}
