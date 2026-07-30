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
)
