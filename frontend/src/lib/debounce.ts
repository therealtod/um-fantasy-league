/**
 * Wraps `fn` so a burst of calls collapses into one, fired `waitMs` after the
 * last call in the burst rather than on every call. Used to keep a per-
 * keystroke handler (e.g. a search box) from issuing a request per key.
 */
export function debounce<Args extends unknown[]>(
  fn: (...args: Args) => void,
  waitMs: number,
): (...args: Args) => void {
  let timer: ReturnType<typeof setTimeout> | undefined

  return (...args: Args) => {
    clearTimeout(timer)
    timer = setTimeout(() => fn(...args), waitMs)
  }
}
