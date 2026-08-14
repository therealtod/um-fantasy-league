const CREDITS_FORMATTER = new Intl.NumberFormat('en-US')

/**
 * Formats a credit amount the way the design system means it: `CR`, not `$` — the tournament's
 * budget unit is `tournament.credit_grant`, not a real-world currency. Negative values (e.g.
 * `BudgetStatus.remaining` when over budget) render with a leading `-` rather than parentheses.
 */
export function formatCredits(value: number): string {
  const sign = value < 0 ? '-' : ''
  return `${sign}${CREDITS_FORMATTER.format(Math.abs(value))} CR`
}
