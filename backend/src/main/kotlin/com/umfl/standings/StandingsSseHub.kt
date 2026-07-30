package com.umfl.standings

import com.umfl.common.ServiceUnavailableException
import jakarta.annotation.PreDestroy
import org.slf4j.LoggerFactory
import org.springframework.stereotype.Component
import org.springframework.transaction.event.TransactionPhase
import org.springframework.transaction.event.TransactionalEventListener
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Holds one SSE connection per browser tab watching a tournament's standings
 * and pushes a bare "something changed" signal after a match write commits —
 * the client already knows how to pull fresh data via the existing REST
 * endpoints, so the payload here is deliberately just the tournament id.
 *
 * The keep-alive executor is the one narrow exception to "no scheduled tasks"
 * (see AGENTS.md): it exists purely to hold idle connections open through
 * proxy/browser timeouts, not to run business logic.
 */
@Component
class StandingsSseHub {

    private val log = LoggerFactory.getLogger(javaClass)
    private val emittersByTournament = ConcurrentHashMap<Long, CopyOnWriteArrayList<SseEmitter>>()

    private val subscribeLock = Any()
    private var totalSubscribers = 0

    private val keepAlive = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "standings-sse-keepalive").apply { isDaemon = true }
    }

    init {
        keepAlive.scheduleAtFixedRate(::sendKeepAlive, KEEP_ALIVE_SECONDS, KEEP_ALIVE_SECONDS, TimeUnit.SECONDS)
    }

    fun subscribe(tournamentId: Long): SseEmitter {
        val emitter: SseEmitter
        val emitters: CopyOnWriteArrayList<SseEmitter>
        synchronized(subscribeLock) {
            val existing = emittersByTournament[tournamentId]
            if (totalSubscribers >= MAX_TOTAL_SUBSCRIBERS ||
                (existing?.size ?: 0) >= MAX_SUBSCRIBERS_PER_TOURNAMENT
            ) {
                throw ServiceUnavailableException("The standings stream is at capacity; please retry in a moment.")
            }
            // Finite backstop, not relied on for normal cleanup — the keep-alive heartbeat and
            // client-side reconnect (with capped backoff) already handle the common cases. This
            // just guarantees no emitter (leaked, stuck, or otherwise) outlives an hour, bounding
            // the worst case instead of leaving it open forever.
            emitter = SseEmitter(EMITTER_TIMEOUT_MILLIS)
            emitters = emittersByTournament.computeIfAbsent(tournamentId) { CopyOnWriteArrayList() }
            emitters.add(emitter)
            totalSubscribers++
        }

        val release = { releaseEmitter(emitters, emitter) }
        emitter.onCompletion(release)
        // Completing here (rather than just releasing bookkeeping) matters: SseEmitter is backed by
        // a DeferredResult, and if a timeout elapses without the app setting a result, Spring dispatches
        // an AsyncRequestTimeoutException back through the normal MVC pipeline onto a response whose
        // Content-Type is already committed to text/event-stream — GlobalExceptionHandler then fails
        // trying to write a ProblemDetail body with an incompatible preset content type.
        emitter.onTimeout { emitter.complete() }
        emitter.onError { release() }

        return emitter
    }

    @TransactionalEventListener(phase = TransactionPhase.AFTER_COMMIT)
    fun onStandingsUpdate(event: StandingsUpdateEvent) {
        val emitters = emittersByTournament[event.tournamentId] ?: return
        for (emitter in emitters) {
            send(emitter, emitters) { it.send(SseEmitter.event().name("update").data(event.tournamentId)) }
        }
    }

    private fun sendKeepAlive() {
        for ((_, emitters) in emittersByTournament) {
            for (emitter in emitters) {
                send(emitter, emitters) { it.send(SseEmitter.event().comment("keep-alive")) }
            }
        }
    }

    private fun send(emitter: SseEmitter, emitters: CopyOnWriteArrayList<SseEmitter>, action: (SseEmitter) -> Unit) {
        try {
            action(emitter)
        } catch (e: IOException) {
            releaseEmitter(emitters, emitter)
        } catch (e: IllegalStateException) {
            // Emitter already completed/timed out concurrently — its own callback removes it.
            log.debug("SSE emitter already closed", e)
        }
    }

    /**
     * Single removal path for all four sites an emitter can drop out from
     * (onCompletion, onTimeout, onError, a failed push in [send]) — `onCompletion`
     * always fires in addition to whichever of the other three also fires, so
     * without this, `totalSubscribers` would be decremented more than once per
     * emitter. `List.remove`'s boolean return (true only the first time) is the
     * idempotency check.
     */
    private fun releaseEmitter(emitters: CopyOnWriteArrayList<SseEmitter>, emitter: SseEmitter) {
        if (emitters.remove(emitter)) {
            synchronized(subscribeLock) { totalSubscribers-- }
        }
    }

    @PreDestroy
    fun shutdown() {
        keepAlive.shutdownNow()
    }

    /** Test seam: real callers have no reason to know how many tabs are watching. */
    internal fun subscriberCount(tournamentId: Long): Int = emittersByTournament[tournamentId]?.size ?: 0

    /** Test seam: same rationale as [subscriberCount]. */
    internal fun totalSubscriberCount(): Int = totalSubscribers

    companion object {
        const val KEEP_ALIVE_SECONDS = 20L
        const val MAX_SUBSCRIBERS_PER_TOURNAMENT = 200
        const val MAX_TOTAL_SUBSCRIBERS = 500
        const val EMITTER_TIMEOUT_MILLIS = 60L * 60 * 1000 // 1 hour
    }
}
