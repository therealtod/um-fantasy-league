package com.umfl.ratelimit

import org.springframework.boot.context.properties.ConfigurationProperties
import org.springframework.stereotype.Component
import java.time.Duration

/**
 * Operational tuning for [RateLimitFilter] — how aggressively a deployed
 * instance throttles, not domain data. See the `rate-limit.api` comment in
 * `application.yml` for why this doesn't count as one of the `umfl.*`
 * tunables this codebase otherwise avoids.
 */
@Component
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
