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
)
