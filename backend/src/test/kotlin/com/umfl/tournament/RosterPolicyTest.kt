package com.umfl.tournament

import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import java.time.Instant
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The roster rules, exercised directly.
 *
 * Cost literals are the seeded `tournament_hero` prices for Winter of
 * Champions, so a change to the seed that breaks the "one premium plus two
 * budget picks just fits" tuning shows up here as well as in the integration
 * tests.
 */
class RosterPolicyTest {

    private fun tournament(
        status: TournamentStatus = TournamentStatus.REGISTRATION_OPEN,
        rosterSize: Int = 3,
        creditGrant: Int = 10_000,
    ) = Tournament(
        id = 2L,
        name = "Winter of Champions",
        format = TournamentFormat.ARSENAL,
        status = status,
        startDate = LocalDate.parse("2026-08-14"),
        endDate = null,
        capacity = 64,
        rosterSize = rosterSize,
        creditGrant = creditGrant,
    )

    private fun entry(
        status: EntryStatus = EntryStatus.DRAFT,
        creditGrant: Int = 10_000,
    ) = TournamentEntry(
        id = 10L,
        tournamentId = 2L,
        managerId = 1L,
        status = status,
        creditGrant = creditGrant,
        registeredAt = Instant.parse("2026-07-30T09:00:00Z"),
        lockedAt = if (status == EntryStatus.LOCKED) Instant.parse("2026-08-01T09:00:00Z") else null,
    )

    private fun picks(vararg costs: Int) =
        costs.mapIndexed { index, cost -> RosterPick(heroId = (index + 1).toLong(), cost = cost) }

    @Nested
    inner class Budget {

        @Test
        fun `an empty roster has spent nothing`() {
            val budget = RosterPolicy.budgetStatus(emptyList(), creditGrant = 10_000)

            assertEquals(0, budget.spent)
            assertEquals(10_000, budget.creditGrant)
            assertEquals(10_000, budget.remaining)
            assertEquals(0.0, budget.utilisation)
        }

        @Test
        fun `a partial roster reports what is left`() {
            val budget = RosterPolicy.budgetStatus(picks(4_700), creditGrant = 10_000)

            assertEquals(4_700, budget.spent)
            assertEquals(5_300, budget.remaining)
            assertEquals(0.47, budget.utilisation)
        }

        @Test
        fun `the seeded legal trio just fits the grant`() {
            // Alice 4100 + Robin Hood 3200 + Bigfoot 2100.
            val budget = RosterPolicy.budgetStatus(picks(4_100, 3_200, 2_100), creditGrant = 10_000)

            assertEquals(9_400, budget.spent)
            assertEquals(600, budget.remaining)
            assertEquals(0.94, budget.utilisation)
        }

        @Test
        fun `the seeded premium trio busts it`() {
            // Sun Wukong 5300 + Medusa 5600 + King Arthur 4700, at Winter prices.
            val budget = RosterPolicy.budgetStatus(picks(5_300, 5_600, 4_700), creditGrant = 10_000)

            assertEquals(15_600, budget.spent)
            assertEquals(-5_600, budget.remaining)
            assertEquals(1.56, budget.utilisation)
        }

        @Test
        fun `spending the grant to the last credit is not over budget`() {
            val budget = RosterPolicy.budgetStatus(picks(10_000), creditGrant = 10_000)

            assertEquals(0, budget.remaining)
            assertEquals(1.0, budget.utilisation)
        }
    }

    @Nested
    inner class DraftValidation {

        @Test
        fun `a partial roster is a perfectly good draft`() {
            val violations = RosterPolicy.validateDraft(picks(4_700), tournament(), EntryStatus.DRAFT)

            assertTrue(violations.isEmpty(), "expected no violations but got $violations")
        }

        @Test
        fun `going over budget is allowed while drafting`() {
            // A draft is a scratchpad; the budget bites at lock time.
            val violations =
                RosterPolicy.validateDraft(picks(5_300, 5_600, 4_700), tournament(), EntryStatus.DRAFT)

            assertTrue(violations.isEmpty(), "expected no violations but got $violations")
        }

        @Test
        fun `selecting more heroes than the roster size is rejected`() {
            val violations =
                RosterPolicy.validateDraft(picks(1_000, 1_000, 1_000, 1_000), tournament(), EntryStatus.DRAFT)

            assertEquals(listOf(RosterRule.TOO_MANY_PICKS), violations.map { it.rule })
        }

        @Test
        fun `the same hero cannot be picked twice`() {
            val duplicated = listOf(RosterPick(heroId = 7L, cost = 2_100), RosterPick(heroId = 7L, cost = 2_100))

            val violations = RosterPolicy.validateDraft(duplicated, tournament(), EntryStatus.DRAFT)

            assertEquals(listOf(RosterRule.DUPLICATE_HERO), violations.map { it.rule })
            assertTrue(violations.single().message.contains("7"))
        }

        @Test
        fun `a locked entry cannot be edited`() {
            val violations = RosterPolicy.validateDraft(picks(1_000), tournament(), EntryStatus.LOCKED)

            assertEquals(listOf(RosterRule.ENTRY_LOCKED), violations.map { it.rule })
        }

        @Test
        fun `rosters freeze once the tournament is live`() {
            val violations = RosterPolicy.validateDraft(
                picks(1_000),
                tournament(status = TournamentStatus.LIVE),
                EntryStatus.DRAFT,
            )

            assertEquals(listOf(RosterRule.TOURNAMENT_CLOSED), violations.map { it.rule })
        }

        @Test
        fun `every broken rule is reported, not just the first`() {
            val duplicated = listOf(
                RosterPick(heroId = 7L, cost = 1_000),
                RosterPick(heroId = 7L, cost = 1_000),
                RosterPick(heroId = 8L, cost = 1_000),
                RosterPick(heroId = 9L, cost = 1_000),
            )

            val violations = RosterPolicy.validateDraft(
                duplicated,
                tournament(status = TournamentStatus.COMPLETED),
                EntryStatus.LOCKED,
            )

            assertEquals(
                setOf(
                    RosterRule.ENTRY_LOCKED,
                    RosterRule.TOURNAMENT_CLOSED,
                    RosterRule.TOO_MANY_PICKS,
                    RosterRule.DUPLICATE_HERO,
                ),
                violations.map { it.rule }.toSet(),
            )
        }
    }

    @Nested
    inner class LockValidation {

        @Test
        fun `a full roster within budget locks cleanly`() {
            val violations = RosterPolicy.validateLock(picks(4_100, 3_200, 2_100), tournament(), entry())

            assertTrue(violations.isEmpty(), "expected no violations but got $violations")
        }

        @Test
        fun `a partial roster cannot be locked`() {
            val violations = RosterPolicy.validateLock(picks(4_700, 3_200), tournament(), entry())

            assertEquals(listOf(RosterRule.INCOMPLETE_ROSTER), violations.map { it.rule })
        }

        @Test
        fun `an over-budget roster cannot be locked`() {
            val violations = RosterPolicy.validateLock(picks(5_300, 5_600, 4_700), tournament(), entry())

            assertEquals(listOf(RosterRule.BUDGET_EXCEEDED), violations.map { it.rule })
            assertTrue(violations.single().message.contains("5600"), violations.single().message)
        }

        @Test
        fun `spending the grant to the last credit is allowed`() {
            val violations = RosterPolicy.validateLock(picks(5_000, 3_000, 2_000), tournament(), entry())

            assertTrue(violations.isEmpty(), "expected no violations but got $violations")
        }

        @Test
        fun `an already-locked roster cannot be locked again`() {
            val violations = RosterPolicy.validateLock(
                picks(4_100, 3_200, 2_100),
                tournament(),
                entry(status = EntryStatus.LOCKED),
            )

            assertEquals(listOf(RosterRule.ENTRY_LOCKED), violations.map { it.rule })
        }

        @Test
        fun `an incomplete over-budget roster reports both problems`() {
            val violations = RosterPolicy.validateLock(picks(9_000, 9_000), tournament(), entry())

            assertEquals(
                setOf(RosterRule.INCOMPLETE_ROSTER, RosterRule.BUDGET_EXCEEDED),
                violations.map { it.rule }.toSet(),
            )
        }

        @Test
        fun `the budget comes from the entry, not the tournament`() {
            // The tournament's grant was raised to 20,000 after this manager
            // registered on 10,000 — their snapshot is what binds.
            val generousNow = tournament(creditGrant = 20_000)

            val violations = RosterPolicy.validateLock(
                picks(6_000, 6_000, 6_000),
                generousNow,
                entry(creditGrant = 10_000),
            )

            assertEquals(listOf(RosterRule.BUDGET_EXCEEDED), violations.map { it.rule })
            assertTrue(violations.single().message.contains("10000"), violations.single().message)
        }

        @Test
        fun `roster size comes from the tournament, not a constant`() {
            val fiveHeroLeague = tournament(rosterSize = 5)

            assertTrue(
                RosterPolicy.validateLock(
                    picks(2_000, 2_000, 2_000, 2_000, 2_000),
                    fiveHeroLeague,
                    entry(),
                ).isEmpty()
            )
            assertFalse(
                RosterPolicy.validateLock(picks(2_000, 2_000, 2_000), fiveHeroLeague, entry()).isEmpty()
            )
        }
    }
}
