package com.umfl.tournament

enum class RosterRule {
    /** The entry is locked; its roster can no longer change. */
    ENTRY_LOCKED,

    /** The tournament has started or finished; rosters are frozen. */
    TOURNAMENT_CLOSED,

    /** More heroes selected than the tournament's roster size allows. */
    TOO_MANY_PICKS,

    /** Fewer heroes than the roster size — cannot lock a partial roster. */
    INCOMPLETE_ROSTER,

    /** The same hero appears more than once. */
    DUPLICATE_HERO,

    /** A selected hero id is not in this tournament's hero pool. */
    UNKNOWN_HERO,

    /** Combined cost is over the entry's credit grant. */
    BUDGET_EXCEEDED,
}

data class RosterViolation(val rule: RosterRule, val message: String)

/**
 * Where a roster's spend sits against its budget — drives the Roster Builder
 * meter.
 *
 * [remaining] goes negative when the roster is over budget, and [utilisation]
 * past 1.0; there are no bands, because "how full is the bar" is the whole
 * question the meter answers.
 */
data class BudgetStatus(
    val spent: Int,
    val creditGrant: Int,
    val remaining: Int,
    val utilisation: Double,
)

/** A prospective roster pick: which hero, and what it costs in this tournament. */
data class RosterPick(val heroId: Long, val cost: Int)

/**
 * The league's roster rules, as pure functions.
 *
 * Deliberately free of Spring and persistence so the rules can be exercised
 * directly in tests. `frontend/src/domain/rosterPolicy.ts` mirrors
 * [budgetStatus] so the meter reacts on click, but this side is authoritative.
 */
object RosterPolicy {

    fun budgetStatus(picks: List<RosterPick>, creditGrant: Int): BudgetStatus {
        require(creditGrant > 0) { "Credit grant must be positive" }
        val spent = picks.sumOf { it.cost }
        return BudgetStatus(
            spent = spent,
            creditGrant = creditGrant,
            remaining = creditGrant - spent,
            utilisation = spent.toDouble() / creditGrant,
        )
    }

    /**
     * Rules that apply while a roster is still being drafted.
     *
     * Going over budget is deliberately *not* a violation here: a draft is a
     * scratchpad, and the Roster Builder shows a negative meter rather than
     * refusing the edit. The budget is enforced at [validateLock].
     *
     * Every broken rule is reported, not just the first, so the UI can highlight
     * everything wrong in one pass.
     */
    fun validateDraft(
        picks: List<RosterPick>,
        tournament: Tournament,
        entryStatus: EntryStatus,
    ): List<RosterViolation> = buildList {
        addAll(mutabilityViolations(tournament, entryStatus))

        if (picks.size > tournament.rosterSize) {
            add(
                RosterViolation(
                    RosterRule.TOO_MANY_PICKS,
                    "Selected ${picks.size} heroes but the roster size is ${tournament.rosterSize}.",
                )
            )
        }

        val duplicates = picks.groupingBy { it.heroId }.eachCount().filterValues { it > 1 }.keys
        if (duplicates.isNotEmpty()) {
            add(
                RosterViolation(
                    RosterRule.DUPLICATE_HERO,
                    "A hero may only be selected once (repeated ids: ${duplicates.sorted().joinToString()}).",
                )
            )
        }
    }

    /**
     * Everything [validateDraft] checks, plus the rules that only bite at commit
     * time.
     *
     * The budget comes from the *entry*, not the tournament: the grant was
     * snapshotted at registration and is what this manager actually has to spend.
     */
    fun validateLock(
        picks: List<RosterPick>,
        tournament: Tournament,
        entry: TournamentEntry,
    ): List<RosterViolation> = buildList {
        addAll(validateDraft(picks, tournament, entry.status))

        if (picks.size < tournament.rosterSize) {
            add(
                RosterViolation(
                    RosterRule.INCOMPLETE_ROSTER,
                    "Roster needs ${tournament.rosterSize} heroes but only ${picks.size} selected.",
                )
            )
        }

        val budget = budgetStatus(picks, entry.creditGrant)
        if (budget.spent > budget.creditGrant) {
            add(
                RosterViolation(
                    RosterRule.BUDGET_EXCEEDED,
                    "Roster costs ${budget.spent} credits, exceeding the ${budget.creditGrant} grant " +
                        "by ${-budget.remaining}.",
                )
            )
        }
    }

    private fun mutabilityViolations(
        tournament: Tournament,
        entryStatus: EntryStatus,
    ): List<RosterViolation> = buildList {
        if (entryStatus == EntryStatus.LOCKED) {
            add(RosterViolation(RosterRule.ENTRY_LOCKED, "This roster is locked and can no longer be changed."))
        }
        if (!tournament.acceptsRosterChanges) {
            add(
                RosterViolation(
                    RosterRule.TOURNAMENT_CLOSED,
                    "${tournament.name} is ${tournament.status} and no longer accepts roster changes.",
                )
            )
        }
    }
}
