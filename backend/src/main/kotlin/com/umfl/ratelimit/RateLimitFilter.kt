package com.umfl.ratelimit

import com.github.benmanes.caffeine.cache.Caffeine
import io.github.bucket4j.Bandwidth
import io.github.bucket4j.Bucket
import jakarta.servlet.FilterChain
import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.springframework.http.HttpHeaders
import org.springframework.http.HttpStatus
import org.springframework.http.MediaType
import org.springframework.http.ProblemDetail
import org.springframework.security.web.util.matcher.IpAddressMatcher
import org.springframework.stereotype.Component
import org.springframework.web.filter.OncePerRequestFilter
import tools.jackson.databind.json.JsonMapper
import java.net.URI
import java.time.Duration

/**
 * IP-keyed token bucket in front of every `/api` route, registered in both
 * [com.umfl.config.SecurityConfig] and [com.umfl.config.DevSecurityConfig] so
 * it runs ahead of authentication in every profile — a flood shouldn't pay
 * JWT verification (or the dev manager lookup) either.
 *
 * Keyed on the client address resolved by [clientIp]: normally
 * [HttpServletRequest.getRemoteAddr], since a forwarded-for header from a
 * peer that could itself be the attacker is worthless. Only when the peer is
 * inside [RateLimitProperties.trustedProxies] — a TLS-terminating reverse
 * proxy on the same host — is `X-Forwarded-For` read instead, because
 * otherwise every request on that topology arrives from the proxy and the
 * whole internet shares one bucket.
 *
 * The tradeoff the original direct-exposure design accepted still stands one
 * hop further out: traffic reaching the proxy through the Cloudflare Worker
 * in `frontend/src/worker.ts` shares a bucket per Cloudflare edge IP rather
 * than per real visitor.
 *
 * The key space is "every IP on the internet that touches `/api/`", which is
 * unbounded, so the bucket store itself must be bounded and self-evicting —
 * a plain `ConcurrentHashMap` would let a port scan grow this without limit
 * for the JVM's lifetime. Caffeine expires an IP's bucket once it has been
 * quiet for two refill periods (its bandwidth would have fully refilled
 * anyway, so nothing is lost by starting over) and caps the map at
 * [RateLimitProperties.maxTrackedIps] regardless, evicting least-recently-used
 * entries first.
 */
@Component
class RateLimitFilter(
    private val jsonMapper: JsonMapper,
    private val properties: RateLimitProperties,
) : OncePerRequestFilter() {

    private val buckets = Caffeine.newBuilder()
        .maximumSize(properties.maxTrackedIps)
        .expireAfterAccess(properties.refillPeriod.multipliedBy(2))
        .build<String, Bucket> { newBucket() }

    private val trustedProxies = properties.trustedProxies.map(::IpAddressMatcher)

    override fun doFilterInternal(
        request: HttpServletRequest,
        response: HttpServletResponse,
        filterChain: FilterChain,
    ) {
        if (!request.requestURI.startsWith("/api/")) {
            filterChain.doFilter(request, response)
            return
        }

        val bucket = buckets.get(clientIp(request))
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

    /**
     * The address to charge this request to.
     *
     * Reads the **last** `X-Forwarded-For` entry, not the first, and only
     * from a trusted peer. A proxy *appends* the address it saw to whatever
     * the client sent, so the last entry is the one our own proxy observed
     * and the only one it vouches for; the earlier entries are client-supplied
     * and a flooder would happily rotate a fake first entry per request to get
     * a fresh bucket each time. An untrusted peer is the client, header or no
     * header.
     */
    private fun clientIp(request: HttpServletRequest): String {
        val peer = request.remoteAddr
        if (trustedProxies.none { it.matches(peer) }) return peer

        val forwarded = request.getHeader(X_FORWARDED_FOR) ?: return peer
        return forwarded.substringAfterLast(',').trim().removeSurrounding("[", "]").ifEmpty { peer }
    }

    private fun newBucket(): Bucket {
        val bandwidth = Bandwidth.builder()
            .capacity(properties.capacity)
            .refillGreedy(properties.capacity, properties.refillPeriod)
            .build()
        return Bucket.builder().addLimit(bandwidth).build()
    }

    private companion object {
        // Not on Spring's HttpHeaders, unlike the response constants above.
        const val X_FORWARDED_FOR = "X-Forwarded-For"
    }
}
