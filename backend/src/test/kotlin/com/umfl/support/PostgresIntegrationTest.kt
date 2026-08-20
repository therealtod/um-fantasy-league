package com.umfl.support

import com.umfl.match.MatchResultCache
import org.junit.jupiter.api.BeforeEach
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.test.context.SpringBootTest
import org.springframework.boot.testcontainers.service.connection.ServiceConnection
import org.springframework.test.context.ActiveProfiles
import org.springframework.transaction.annotation.Transactional
import org.testcontainers.postgresql.PostgreSQLContainer

/**
 * Base class for tests that need a real PostgreSQL.
 *
 * The container is started once here and shared by every subclass in the run.
 * Flyway migrates it on context startup, which means these tests also serve as
 * the check that the migrations themselves are valid.
 *
 * Each test runs in a transaction that is rolled back afterwards, so subclasses
 * can mutate the seeded data freely without ordering themselves around each
 * other.
 *
 * Deliberately *not* `@Testcontainers` + `@Container`: that extension stops a
 * static container in `afterAll` of every class it is applied to, so the second
 * test class onwards would inherit a cached Spring context still pointing at the
 * dead container's port. Starting it by hand ties its life to the JVM instead --
 * the Ryuk sidecar removes it when the JVM exits.
 *
 * Requires a running Docker daemon.
 */
@SpringBootTest
@ActiveProfiles("test")
@Transactional
abstract class PostgresIntegrationTest {

    @Autowired
    private lateinit var matchResultCache: MatchResultCache

    /**
     * A safety net, not the mechanism.
     *
     * [MatchResultCache] is a singleton in the context every test class shares
     * while these tests roll back, so a test that reads the standings after
     * writing could leave it holding a list assembled from data the rollback
     * then threw away. The cache handles that itself: its invalidation listener
     * is `AFTER_COMPLETION` rather than `AFTER_COMMIT` precisely so a rollback
     * clears it as surely as a commit would.
     *
     * What that does not cover is a test which mutates `tournament_match` or its
     * children by raw SQL, publishing nothing — `SchemaAndSeedTest` inserts into
     * `match_game` directly. Clearing here makes a cross-test leak structurally
     * impossible in whatever order JUnit runs things, rather than contingent on
     * every test remembering to write through a service. It is deliberately not
     * the same as disabling the cache under `test`: every test below still
     * exercises the real cached read path.
     */
    @BeforeEach
    fun clearMatchResultCache() {
        matchResultCache.invalidateAll()
    }

    companion object {
        @ServiceConnection
        @JvmStatic
        val postgres: PostgreSQLContainer =
            PostgreSQLContainer("postgres:17-alpine").apply { start() }
    }
}
