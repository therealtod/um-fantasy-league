package com.umfl.ratelimit

import org.springframework.boot.context.properties.ConfigurationProperties
import java.time.Duration

/**
 * Operational tuning for [RateLimitFilter] — how aggressively a deployed
 * instance throttles, not domain data. See the `rate-limit.api` comment in
 * `application.yml` for why this doesn't count as one of the `umfl.*`
 * tunables this codebase otherwise avoids.
 *
 * Registered via `@ConfigurationPropertiesScan` on the application class, not
 * `@Component`: this is a Kotlin `data class` of `val`s with no setters, and
 * `@Component` makes Spring bind properties the JavaBean (setter) way rather
 * than via the constructor. That only breaks once a value is actually
 * supplied from the environment — which no deployment happens to do for this
 * particular class, so it never surfaced here the way it did for
 * [com.umfl.matchimport.ScraperProperties], whose `base-url` docker-compose
 * always overrides. `@ConfigurationPropertiesScan` gets proper constructor
 * binding instead, for both.
 */
@ConfigurationProperties(prefix = "rate-limit.api")
data class RateLimitProperties(
    val capacity: Long = 300,
    val refillPeriod: Duration = Duration.ofMinutes(1),
    /**
     * Upper bound on distinct IPs tracked at once — the key space is "every
     * IP that touches `/api/`", so [RateLimitFilter]'s bucket cache must cap
     * itself rather than grow for the JVM's lifetime. Least-recently-used
     * entries are evicted first once this is exceeded.
     */
    val maxTrackedIps: Long = 100_000,
    /**
     * CIDR ranges whose `X-Forwarded-For` [RateLimitFilter] will believe. A
     * peer outside every range is treated as the client itself and its header
     * ignored, so this list is the whole trust boundary — see the filter's
     * KDoc for why the *last* entry is the one read.
     *
     * The defaults cover a reverse proxy on the same host. Loopback alone is
     * not enough: publishing the container as `127.0.0.1:8080:8080` still
     * NATs the connection through the Docker bridge, so the proxy arrives as
     * the bridge gateway (`172.17.0.1`, or the compose network's equivalent)
     * rather than `127.0.0.1`. These ranges are private and unroutable, so a
     * backend exposed directly on a public interface — the topology this
     * filter was originally written for — never matches one and keeps its
     * old behaviour unchanged.
     */
    val trustedProxies: List<String> = listOf(
        "127.0.0.1/32",
        "::1/128",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
    ),
)
