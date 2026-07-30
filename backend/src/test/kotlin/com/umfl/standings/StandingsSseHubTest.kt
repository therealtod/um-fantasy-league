package com.umfl.standings

import com.umfl.common.ServiceUnavailableException
import org.junit.jupiter.api.AfterEach
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

/**
 * No Spring context needed. `SseEmitter`'s real send/complete machinery only
 * activates once Spring MVC attaches its (package-private) `Handler` to a live
 * HTTP response, which a plain unit test can't do — so this covers the part
 * that *is* observable in isolation: the per-tournament subscriber registry.
 * The actual push-on-commit and disconnect-pruning behavior is exercised by
 * the manual two-tab verification described in the plan.
 */
class StandingsSseHubTest {

    private val hub = StandingsSseHub()

    @AfterEach
    fun tearDown() {
        hub.shutdown()
    }

    @Test
    fun `subscribing registers exactly one emitter for that tournament`() {
        hub.subscribe(7L)

        assertEquals(1, hub.subscriberCount(7L))
    }

    @Test
    fun `subscriptions for different tournaments are kept separate`() {
        hub.subscribe(7L)
        hub.subscribe(7L)
        hub.subscribe(99L)

        assertEquals(2, hub.subscriberCount(7L))
        assertEquals(1, hub.subscriberCount(99L))
    }

    @Test
    fun `an update for a tournament with no subscribers is a no-op`() {
        hub.onStandingsUpdate(StandingsUpdateEvent(404L))

        assertEquals(0, hub.subscriberCount(404L))
    }

    @Test
    fun `an update for a subscribed tournament does not throw`() {
        hub.subscribe(7L)

        hub.onStandingsUpdate(StandingsUpdateEvent(7L))

        assertEquals(1, hub.subscriberCount(7L))
    }

    @Test
    fun `subscribing past the per-tournament cap throws and does not register the emitter`() {
        repeat(StandingsSseHub.MAX_SUBSCRIBERS_PER_TOURNAMENT) { hub.subscribe(7L) }

        assertFailsWith<ServiceUnavailableException> { hub.subscribe(7L) }
        assertEquals(StandingsSseHub.MAX_SUBSCRIBERS_PER_TOURNAMENT, hub.subscriberCount(7L))
    }

    @Test
    fun `the per-tournament cap on one tournament does not block a different tournament`() {
        repeat(StandingsSseHub.MAX_SUBSCRIBERS_PER_TOURNAMENT) { hub.subscribe(7L) }
        assertFailsWith<ServiceUnavailableException> { hub.subscribe(7L) }

        hub.subscribe(99L)

        assertEquals(1, hub.subscriberCount(99L))
    }

    @Test
    fun `subscribing past the total cap throws even for a fresh tournament well under its own per-tournament cap`() {
        var tournamentId = 1L
        while (hub.totalSubscriberCount() < StandingsSseHub.MAX_TOTAL_SUBSCRIBERS) {
            val remaining = StandingsSseHub.MAX_TOTAL_SUBSCRIBERS - hub.totalSubscriberCount()
            val toAdd = minOf(remaining, StandingsSseHub.MAX_SUBSCRIBERS_PER_TOURNAMENT)
            repeat(toAdd) { hub.subscribe(tournamentId) }
            tournamentId++
        }

        assertFailsWith<ServiceUnavailableException> { hub.subscribe(tournamentId) }
    }
}
