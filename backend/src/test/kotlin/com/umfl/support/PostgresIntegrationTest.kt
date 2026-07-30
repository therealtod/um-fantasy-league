package com.umfl.support

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

    companion object {
        @ServiceConnection
        @JvmStatic
        val postgres: PostgreSQLContainer =
            PostgreSQLContainer("postgres:17-alpine").apply { start() }
    }
}
