/**
 * Case-insensitive alphabetical comparator for anything with a `name` field.
 * Pass directly to `Array.prototype.sort`.
 */
export function byName(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
}
