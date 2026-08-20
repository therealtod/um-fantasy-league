package com.umfl.match

import com.umfl.standings.StandingsUpdateEvent
import org.springframework.jdbc.core.JdbcTemplate
import org.springframework.jdbc.core.simple.JdbcClient
import java.time.Instant
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame
import kotlin.test.assertTrue

/**
 * Pure unit tests — no Spring, no Postgres. [MatchResultQuery] is subclassed
 * with a counting stub so the tests can assert exactly how many reads the cache
 * let through, which is the only thing it exists to control.
 */
class MatchResultCacheTest {

    /**
     * Counts loads and lets a test block inside one, which is what the
     * invalidate-during-load race needs to be reproducible rather than timing
     * dependent. The `JdbcClient` handed to the superclass wraps a
     * datasource-less [JdbcTemplate] and is never used: [findByTournament] is
     * the only method these tests reach and it is overridden here.
     */
    private open class CountingQuery(
        private val onLoad: (Long) -> List<MatchResult> = { emptyList() },
    ) : MatchResultQuery(JdbcClient.create(JdbcTemplate())) {

        val loads = AtomicInteger()

        override fun findByTournament(tournamentId: Long, round: Int?): List<MatchResult> {
            loads.incrementAndGet()
            return onLoad(tournamentId)
        }
    }

    private fun match(id: Long) = MatchResult(
        matchId = id,
        tournamentId = 1,
        round = 1,
        playedAt = Instant.EPOCH,
        externalLink = "urn:umfl:match:$id",
        participants = emptyList(),
        games = emptyList(),
        bans = emptyList(),
    )

    @Test
    fun `a second read is served from the cache`() {
        val query = CountingQuery { listOf(match(1)) }
        val cache = MatchResultCache(query)

        val first = cache.findByTournament(1)
        val second = cache.findByTournament(1)

        assertEquals(1, query.loads.get(), "the second read should not have queried")
        assertSame(first, second)
    }

    @Test
    fun `each tournament is cached separately`() {
        val query = CountingQuery { listOf(match(it)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.findByTournament(2)
        cache.findByTournament(1)

        assertEquals(2, query.loads.get())
        assertEquals(2, cache.cachedTournamentCount())
    }

    @Test
    fun `invalidate forces the next read to query again`() {
        val query = CountingQuery { listOf(match(1)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.invalidate(1)
        cache.findByTournament(1)

        assertEquals(2, query.loads.get())
    }

    @Test
    fun `invalidateAll drops every tournament`() {
        val query = CountingQuery { listOf(match(1)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.findByTournament(2)
        cache.invalidateAll()
        cache.findByTournament(1)
        cache.findByTournament(2)

        assertEquals(4, query.loads.get())
    }

    @Test
    fun `a match write invalidates only its own tournament`() {
        val query = CountingQuery { listOf(match(1)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.findByTournament(2)
        cache.onMatchWritePublished(StandingsUpdateEvent(1))
        cache.findByTournament(1)
        cache.findByTournament(2)

        assertEquals(3, query.loads.get(), "tournament 2 should still have been cached")
    }

    @Test
    fun `the after-completion listener invalidates too`() {
        val query = CountingQuery { listOf(match(1)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.onMatchWriteCompleted(StandingsUpdateEvent(1))
        cache.findByTournament(1)

        assertEquals(2, query.loads.get())
    }

    /**
     * The burst the cache exists for: one match write wakes hundreds of tabs,
     * each of which asks for the board and the ticker at once. They must
     * collapse onto a single query rather than each running their own.
     */
    @Test
    fun `concurrent readers of a cold cache load once`() {
        val started = CountDownLatch(1)
        val query = CountingQuery {
            // Hold the load open long enough that every thread is genuinely
            // contending, rather than arriving after the first one finished.
            Thread.sleep(50)
            listOf(match(1))
        }
        val cache = MatchResultCache(query)
        val readers = 32
        val pool = Executors.newFixedThreadPool(readers)

        try {
            val results = (1..readers).map {
                pool.submit<List<MatchResult>> {
                    started.await()
                    cache.findByTournament(1)
                }
            }
            started.countDown()
            results.forEach { assertEquals(listOf(match(1)), it.get(10, TimeUnit.SECONDS)) }
        } finally {
            pool.shutdownNow()
        }

        assertEquals(1, query.loads.get(), "the burst should have collapsed onto one query")
    }

    /**
     * The race the generation stamp exists for. A reader misses and starts
     * loading; a write commits and invalidates the still-empty entry underneath
     * it; the reader then stores a list that is already out of date. Without the
     * stamp that entry survives, because the invalidation it raced has already
     * happened — every later reader would be served the pre-write matches
     * indefinitely.
     */
    @Test
    fun `a load that races an invalidation is not left in the cache`() {
        val loadStarted = CountDownLatch(1)
        val invalidated = CountDownLatch(1)
        val version = AtomicInteger(1)
        val query = CountingQuery {
            loadStarted.countDown()
            // Only the first load waits; the reload afterwards must not block.
            if (version.get() == 1) invalidated.await()
            listOf(match(version.get().toLong()))
        }
        val cache = MatchResultCache(query)
        val pool = Executors.newSingleThreadExecutor()

        try {
            val racing = pool.submit<List<MatchResult>> { cache.findByTournament(1) }
            loadStarted.await(10, TimeUnit.SECONDS)

            // The write lands while the reader is still inside its query.
            version.set(2)
            cache.invalidate(1)
            invalidated.countDown()

            racing.get(10, TimeUnit.SECONDS)

            // The stale list must not have been left behind for the next reader.
            assertEquals(listOf(match(2)), cache.findByTournament(1), "a stale entry survived the race")
            assertTrue(query.loads.get() >= 2, "the racing load should have been re-run")
        } finally {
            pool.shutdownNow()
        }
    }

    /**
     * The same race as above with the threads taken out — the invalidation
     * happens *inside* the load, which is what a write landing mid-query amounts
     * to, and makes the assertion exact rather than merely very likely.
     *
     * Calling [MatchResultCache.invalidate] from within the loader is safe only
     * because it bumps a counter and never touches the entry map; that is the
     * same property that keeps an admin's write off the back of a reader's
     * query in production.
     */
    @Test
    fun `a load invalidated while it runs is discarded and retried`() {
        lateinit var cache: MatchResultCache
        val loads = AtomicInteger()
        val query = CountingQuery { id ->
            // The first load alone is overtaken by a write; later ones are clean.
            if (loads.incrementAndGet() == 1) cache.invalidate(id)
            listOf(match(loads.get().toLong()))
        }
        cache = MatchResultCache(query)

        assertEquals(listOf(match(2)), cache.findByTournament(1), "the overtaken load was published")
        assertEquals(2, query.loads.get())

        // And the discarded entry is not still sitting there for the next reader.
        assertEquals(listOf(match(2)), cache.findByTournament(1))
        assertEquals(2, query.loads.get(), "the retry's result should now be cached")
    }

    /**
     * A cache that can never settle must degrade to the uncached behaviour this
     * change replaced, rather than spinning. It takes concurrent admin writes to
     * reach, which is to say it does not happen — but the bound is what makes
     * that a fact rather than a hope.
     */
    @Test
    fun `an unceasing invalidator degrades to a read-through`() {
        lateinit var cache: MatchResultCache
        val query = CountingQuery { id ->
            cache.invalidate(id)
            listOf(match(1))
        }
        cache = MatchResultCache(query)

        assertEquals(listOf(match(1)), cache.findByTournament(1))
        assertEquals(
            MatchResultCache.MAX_LOAD_ATTEMPTS + 1,
            query.loads.get(),
            "should have given up after the retry budget and read through once",
        )
    }

    @Test
    fun `a rename invalidates every tournament`() {
        val query = CountingQuery { listOf(match(it)) }
        val cache = MatchResultCache(query)

        cache.findByTournament(1)
        cache.findByTournament(2)
        cache.onRenamePublished(ReferenceDataRenamedEvent("hero 7"))
        cache.findByTournament(1)
        cache.findByTournament(2)

        assertEquals(4, query.loads.get())
    }

    /**
     * The ticker's page, as the cache derives it. The exactness against the SQL
     * it replaces is pinned by `StandingsIntegrationTest` against a real
     * database; this covers the slicing arithmetic itself.
     */
    @Test
    fun `the ticker slice reverses, filters and truncates`() {
        val ascending = (1L..5L).map(::match)
        val cache = MatchResultCache(CountingQuery { ascending })

        assertEquals(
            listOf(5L, 4L, 3L, 2L, 1L),
            cache.findByTournamentSince(1, sinceMatchId = 0, limit = 25).map { it.matchId },
            "newest first",
        )
        assertEquals(
            listOf(5L, 4L),
            cache.findByTournamentSince(1, sinceMatchId = 3, limit = 25).map { it.matchId },
            "only matches after sinceMatchId",
        )
        assertEquals(
            listOf(5L, 4L),
            cache.findByTournamentSince(1, sinceMatchId = 0, limit = 2).map { it.matchId },
            "truncated to the newest `limit`",
        )
        assertEquals(
            emptyList(),
            cache.findByTournamentSince(1, sinceMatchId = 99, limit = 25).map { it.matchId },
            "a sinceMatchId past the end yields nothing",
        )
    }
}
