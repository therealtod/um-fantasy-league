package com.umfl.match

import com.github.benmanes.caffeine.cache.Caffeine
import com.umfl.standings.StandingsUpdateEvent
import org.slf4j.LoggerFactory
import org.springframework.context.event.EventListener
import org.springframework.stereotype.Component
import org.springframework.transaction.event.TransactionPhase
import org.springframework.transaction.event.TransactionalEventListener
import java.time.Duration
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * A tournament's whole match list, held in memory between writes — a read-through
 * front for [MatchResultQuery] exposing the same two methods the standings path
 * used to call on it directly.
 *
 * The pressure this relieves is a burst, not a trickle. `GET /standings` is
 * public and unauthenticated, and the SSE hub tells up to
 * [com.umfl.standings.StandingsSseHub.MAX_SUBSCRIBERS_PER_TOURNAMENT] tabs to
 * refetch the instant an admin records a match — each of which then pulls *both*
 * the board and the ticker head. Uncached, one admin keystroke is hundreds of
 * simultaneous requests, every one of them running
 * [MatchResultQuery.findByTournament]'s six unbounded queries against a
 * ten-connection pool.
 *
 * **Only the fold's input is cached, never the board.** Matches have exactly one
 * writer, [AdminMatchService], and it already announces all three of its writes
 * with a [StandingsUpdateEvent] — so this key has an invalidation signal that is
 * complete by construction. Coefficients, hero costs, credit grants and rosters
 * have no such signal (AGENTS.md documents retuning a scoring weight as a bare
 * `UPDATE`), so they stay read live on every request and a re-price, a
 * registration or a slot change is visible on the very next one. Only a *match*
 * can ever be a write behind here.
 *
 * Hand-rolled Caffeine rather than Spring's `@Cacheable`/`@EnableCaching`, as
 * [com.umfl.ratelimit.RateLimitFilter] already does it. `@Cacheable` could not
 * express the version stamp below in any case: its abstraction has "put" and
 * "evict", and the correctness argument here turns on a third operation,
 * "publish this load *only if* nothing invalidated me while it ran".
 *
 * ### Why a version stamp and not a plain evict
 *
 * Eviction alone cannot survive an invalidation that lands mid-load:
 *
 * 1. a reader misses and starts loading;
 * 2. a write commits, and its listener evicts an entry that is not there yet;
 * 3. the reader finishes and stores a list that is already wrong — and that
 *    nothing will ever invalidate again, because the invalidation it needed has
 *    already been and gone.
 *
 * Caffeine's `get(key, loader)` is atomic per key, which is exactly what
 * collapses the burst onto one database read; but that atomicity is also why the
 * loader cannot decline to store — returning null would mean "absent", and every
 * blocked thread would then load for itself and the collapse would be gone. So
 * the entry is always stored, carrying the version its load *started* under, and
 * the reader checks it afterwards. Reading the version before the load rather
 * than after is what makes the check err safely: a spurious mismatch costs one
 * extra load, while stamping afterwards would swallow the very invalidation this
 * is here to catch.
 *
 * ### Why two listeners, in two different phases
 *
 * [onMatchWritePublished] bumps the version the moment the event is published,
 * *inside* the writing transaction. [onMatchWriteCompleted] bumps it again once
 * that transaction ends. Neither is sufficient alone:
 *
 * - without the second, a reader that loads between the publish and the commit
 *   reads pre-write rows, stamps them with the already-bumped version, and that
 *   stale entry then matches forever;
 * - without the first, a reader *inside* the writing transaction is served the
 *   list as it stood before the write — which is what every test extending
 *   `PostgresIntegrationTest` is, since each runs in a transaction that is
 *   rolled back and never commits.
 *
 * [onMatchWriteCompleted] is `AFTER_COMPLETION`, not `AFTER_COMMIT`, and the
 * difference is load-bearing rather than cosmetic. A rollback un-writes rows
 * this cache may already have loaded inside that transaction, so it invalidates
 * just as surely as a commit does; `AFTER_COMMIT` would leave reverted state
 * cached. By contrast [com.umfl.standings.StandingsSseHub.onStandingsUpdate]
 * listens to the same event and is correctly `AFTER_COMMIT`, because telling
 * browsers "something changed" about a write that rolled back would be a lie.
 * Two listeners, two phases, deliberately — don't unify them.
 *
 * ### Bounds
 *
 * Bounded by size and idleness, and deliberately **not** by write age. Unlike
 * [com.umfl.ratelimit.RateLimitFilter], whose key space is every IP on the
 * internet, this one is the `tournament` table — rows only an admin can create —
 * so the bounds are memory hygiene rather than a defence against a hostile key
 * space. There is no `expireAfterWrite`: a TTL cannot add a guarantee the stamp
 * does not already give, and its only effect would be on a *missing* hook, where
 * it converts "this rename never appears" into "this rename appears eventually,
 * depending on when you look" — strictly harder to notice and reproduce. If a
 * hook is ever missing, the fix is the hook.
 *
 * The cached lists are shared across request threads and must stay immutable.
 * [MatchResult] and its children are data classes over read-only lists and
 * nothing downstream mutates them; keep it that way.
 */
@Component
class MatchResultCache(private val matchResultQuery: MatchResultQuery) {

    private val log = LoggerFactory.getLogger(javaClass)

    /**
     * Not a `data class`: this is handed to `remove(key, value)`, where identity
     * is both what is meant — drop the entry *I* just saw — and far cheaper than
     * structurally comparing a few hundred assembled matches.
     */
    private class Stamped(val version: Long, val matches: List<MatchResult>)

    private val cache = Caffeine.newBuilder()
        .maximumSize(MAX_CACHED_TOURNAMENTS)
        .expireAfterAccess(IDLE_EVICTION)
        .build<Long, Stamped>()

    /**
     * One counter per tournament, bumped by every invalidation and never
     * removed — a version that vanished mid-load would take the invalidation
     * with it. Unbounded is right here for the same reason the bounds above are
     * hygiene: the key space is a table only an admin writes to, and an entry is
     * one `AtomicLong`.
     */
    private val versions = ConcurrentHashMap<Long, AtomicLong>()

    /**
     * Every recorded match in the tournament, oldest first — the same contract as
     * [MatchResultQuery.findByTournament] with no round filter.
     *
     * The retry is bounded rather than a `while (true)`: a relentless write
     * stream should degrade to today's behaviour, an uncached read, rather than
     * spin.
     */
    fun findByTournament(tournamentId: Long): List<MatchResult> {
        val version = versionOf(tournamentId)
        repeat(MAX_LOAD_ATTEMPTS) {
            // Read before the load, never after -- see the class doc. Registering
            // the key here, ahead of any load for it, is also what lets
            // invalidateAll's sweep catch a load that is already running.
            val versionBeforeLoad = version.get()
            val entry = cache.get(tournamentId) { id -> Stamped(versionBeforeLoad, load(id)) }
            if (entry.version == version.get()) return entry.matches
            // Loaded across an invalidation. Value-conditional, so an entry
            // another thread has already replaced this one with is never the
            // casualty; every thread blocked on this key sees the same stale
            // value and tries the same removal, and only the first succeeds.
            cache.asMap().remove(tournamentId, entry)
        }
        log.debug("Tournament {} was invalidated under every cache load attempt; reading through.", tournamentId)
        return load(tournamentId)
    }

    /**
     * The newest [limit] matches after [sinceMatchId] — the ticker's page, and
     * the same contract as [MatchResultQuery.findByTournamentSince], served off
     * the cached list instead of a seventh query.
     *
     * The derivation is exact rather than approximate, and rests on one fact:
     * `(played_at, id)` is a total order, since `id` is the primary key and
     * `played_at` is `not null`. So the descending order the ticker's SQL asks
     * for is the precise reverse of the ascending order the cached list is
     * already in, ties included. [asReversed] reverses *the database's own
     * ordering* rather than re-sorting in Kotlin — do not "simplify" this into a
     * `sortedByDescending`, which would reintroduce a comparator that could
     * disagree with Postgres about a timestamp. The sequence is so [take] stops
     * early instead of filtering a whole tournament to hand back forty rows.
     */
    fun findByTournamentSince(tournamentId: Long, sinceMatchId: Long, limit: Int): List<MatchResult> =
        findByTournament(tournamentId)
            .asReversed()
            .asSequence()
            .filter { it.matchId > sinceMatchId }
            .take(limit)
            .toList()

    /**
     * Bumps [tournamentId]'s version, so the next reader reloads.
     *
     * Deliberately does **not** evict. It does not need to — a bumped version
     * already means no reader will accept the entry, and the first one to look
     * removes it on its way past. And evicting here would be actively harmful:
     * Caffeine's `invalidate` has to take the key's lock, so calling it while a
     * reader is mid-load on that key blocks until that reader's query returns.
     * Since invalidation runs on the admin's request thread, that would put an
     * admin's write behind a stranger's database round-trip. An entry nobody
     * ever reads again is reclaimed by [IDLE_EVICTION] instead.
     */
    fun invalidate(tournamentId: Long) {
        versionOf(tournamentId).incrementAndGet()
    }

    /**
     * Bumps every tournament — for a change that is scoped to none of them.
     *
     * Deliberately global rather than targeted. The ids *are* on the cached
     * objects, so a precise invalidation is possible, but it would mean walking
     * every cached match of every tournament to save reloading a handful of
     * lists on an operation that happens when an admin fixes a catalogue typo.
     *
     * The sweep is not atomic and does not need to be: a load already running
     * registered its key in [versions] before starting, so it is bumped too and
     * its entry is rejected on publication. Bumping without evicting, for the
     * reason given on [invalidate].
     */
    fun invalidateAll() {
        versions.values.forEach { it.incrementAndGet() }
    }

    /**
     * Fires immediately, inside the writing transaction; see the class doc for
     * why this is not redundant with [onMatchWriteCompleted].
     *
     * Block bodies on all four listeners on purpose: Spring republishes a
     * non-null value returned from an `@EventListener` as a new event.
     */
    @EventListener
    fun onMatchWritePublished(event: StandingsUpdateEvent) {
        invalidate(event.tournamentId)
    }

    /** Fires once the writing transaction ends — committed *or* rolled back. See the class doc. */
    @TransactionalEventListener(phase = TransactionPhase.AFTER_COMPLETION)
    fun onMatchWriteCompleted(event: StandingsUpdateEvent) {
        invalidate(event.tournamentId)
    }

    /** Same pair, same reasoning, for the renames no match write announces. */
    @EventListener
    fun onRenamePublished(event: ReferenceDataRenamedEvent) {
        log.debug("Invalidating every cached match list: {} was renamed.", event.renamed)
        invalidateAll()
    }

    @TransactionalEventListener(phase = TransactionPhase.AFTER_COMPLETION)
    fun onRenameCompleted(event: ReferenceDataRenamedEvent) {
        invalidateAll()
    }

    private fun load(tournamentId: Long): List<MatchResult> =
        matchResultQuery.findByTournament(tournamentId).also {
            log.debug("Loaded {} match(es) for tournament {} into the match cache.", it.size, tournamentId)
        }

    private fun versionOf(tournamentId: Long): AtomicLong =
        versions.computeIfAbsent(tournamentId) { AtomicLong() }

    /**
     * Test seam: real callers have no reason to know whether a list was cached.
     *
     * Counts entries *present*, not entries valid — [invalidate] leaves a
     * superseded entry in place for the next reader to clear out.
     */
    internal fun cachedTournamentCount(): Long = cache.estimatedSize()

    /**
     * Kotlin constants, not `@ConfigurationProperties`: AGENTS.md's `umfl.*`
     * invariant allows exactly two config blocks, both of them deployment
     * topology a running instance cannot read out of its own database. How many
     * match lists a JVM holds is neither.
     */
    companion object {
        /** Entries, not bytes — a tournament's match list is the largest thing this app assembles. */
        const val MAX_CACHED_TOURNAMENTS = 64L

        /** Idle eviction, not a TTL: a live tournament is re-read constantly and never ages out. */
        val IDLE_EVICTION: Duration = Duration.ofMinutes(30)

        /** See [findByTournament] — a bound on retries, not a correctness knob. */
        const val MAX_LOAD_ATTEMPTS = 3
    }
}
