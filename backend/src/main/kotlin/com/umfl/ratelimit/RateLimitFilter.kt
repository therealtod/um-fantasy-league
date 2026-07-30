package com.umfl.ratelimit

import io.github.bucket4j.Bandwidth
import io.github.bucket4j.Bucket
import jakarta.servlet.FilterChain
import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.springframework.http.HttpHeaders
import org.springframework.http.HttpStatus
import org.springframework.http.MediaType
import org.springframework.http.ProblemDetail
import org.springframework.stereotype.Component
import org.springframework.web.filter.OncePerRequestFilter
import tools.jackson.databind.json.JsonMapper
import java.net.URI
import java.time.Duration
import java.util.concurrent.ConcurrentHashMap

/**
 * IP-keyed token bucket in front of every `/api` route, registered in both
 * [com.umfl.config.SecurityConfig] and [com.umfl.config.DevSecurityConfig] so
 * it runs ahead of authentication in every profile — a flood shouldn't pay
 * JWT verification (or the dev manager lookup) either.
 *
 * Keyed on [HttpServletRequest.getRemoteAddr] rather than `X-Forwarded-For`:
 * the VPS port is reachable directly (no reverse proxy in front of it), so a
 * trusted forwarded-for header would be trivially spoofable by anyone
 * connecting straight to the backend. The tradeoff is that traffic proxied
 * through Cloudflare Pages (see `frontend/public/_redirects`) shares a
 * bucket per Cloudflare edge IP rather than per real visitor.
 */
@Component
class RateLimitFilter(
    private val jsonMapper: JsonMapper,
    private val properties: RateLimitProperties,
) : OncePerRequestFilter() {

    private val buckets = ConcurrentHashMap<String, Bucket>()

    override fun doFilterInternal(request: HttpServletRequest, response: HttpServletResponse, filterChain: FilterChain) {
        if (!request.requestURI.startsWith("/api/")) {
            filterChain.doFilter(request, response)
            return
        }

        val bucket = buckets.computeIfAbsent(request.remoteAddr) { newBucket() }
        val probe = bucket.tryConsumeAndReturnRemaining(1)
        if (probe.isConsumed) {
            filterChain.doFilter(request, response)
            return
        }

        val retryAfterSeconds = Duration.ofNanos(probe.nanosToWaitForRefill).toSeconds() + 1
        response.status = HttpStatus.TOO_MANY_REQUESTS.value()
        response.contentType = MediaType.APPLICATION_PROBLEM_JSON_VALUE
        response.setHeader(HttpHeaders.RETRY_AFTER, retryAfterSeconds.toString())
        val problem = ProblemDetail.forStatus(HttpStatus.TOO_MANY_REQUESTS).apply {
            title = "Too many requests"
            detail = "Rate limit exceeded. Try again later."
            type = URI.create("https://umfl.dev/problems/rate-limit-exceeded")
        }
        jsonMapper.writeValue(response.writer, problem)
    }

    private fun newBucket(): Bucket {
        val bandwidth = Bandwidth.builder()
            .capacity(properties.capacity)
            .refillGreedy(properties.capacity, properties.refillPeriod)
            .build()
        return Bucket.builder().addLimit(bandwidth).build()
    }
}
