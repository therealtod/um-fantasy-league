package com.umfl.ratelimit

import jakarta.servlet.FilterChain
import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.mockito.ArgumentMatchers.any
import org.mockito.Mockito.mock
import org.mockito.Mockito.never
import org.mockito.Mockito.times
import org.mockito.Mockito.verify
import org.mockito.Mockito.`when`
import tools.jackson.databind.json.JsonMapper
import java.io.PrintWriter
import java.io.StringWriter
import java.time.Duration

/**
 * Exercises [RateLimitFilter] in isolation with a tiny hand-built bucket, not
 * the Spring-managed bean's real 300/min capacity from `application.yml` —
 * see [com.umfl.config.SecurityConfigTest] for the wiring-level check that
 * normal request volume never trips the filter.
 */
class RateLimitFilterTest {

    private val jsonMapper = JsonMapper.builder().build()

    private fun newFilter(capacity: Long = 3, maxTrackedIps: Long = 100_000) =
        RateLimitFilter(
            jsonMapper,
            RateLimitProperties(capacity = capacity, refillPeriod = Duration.ofMinutes(1), maxTrackedIps = maxTrackedIps),
        )

    private fun request(uri: String, remoteAddr: String): HttpServletRequest {
        val request = mock(HttpServletRequest::class.java)
        `when`(request.requestURI).thenReturn(uri)
        `when`(request.remoteAddr).thenReturn(remoteAddr)
        return request
    }

    private class FakeResponse {
        val response: HttpServletResponse = mock(HttpServletResponse::class.java)
        val body = StringWriter()

        init {
            `when`(response.writer).thenReturn(PrintWriter(body))
        }
    }

    @Test
    fun `requests within capacity pass through`() {
        val filter = newFilter(capacity = 3)
        val chain = mock(FilterChain::class.java)

        repeat(3) {
            filter.doFilter(request("/api/tournaments", "1.2.3.4"), FakeResponse().response, chain)
        }

        verify(chain, times(3)).doFilter(any(), any())
    }

    @Test
    fun `the request past capacity is rejected with a 429 problem body and Retry-After header`() {
        val filter = newFilter(capacity = 3)
        val chain = mock(FilterChain::class.java)
        repeat(3) { filter.doFilter(request("/api/tournaments", "1.2.3.4"), FakeResponse().response, chain) }

        val blocked = FakeResponse()
        filter.doFilter(request("/api/tournaments", "1.2.3.4"), blocked.response, chain)

        verify(chain, times(3)).doFilter(any(), any())
        verify(blocked.response).status = 429
        verify(blocked.response).contentType = "application/problem+json"
        verify(blocked.response).setHeader(org.mockito.ArgumentMatchers.eq("Retry-After"), org.mockito.ArgumentMatchers.anyString())
        val body = blocked.body.toString()
        assertTrue(body.contains("\"status\":429"))
        assertTrue(body.contains("rate-limit-exceeded"))
    }

    @Test
    fun `different IPs get independent buckets`() {
        val filter = newFilter(capacity = 1)
        val chain = mock(FilterChain::class.java)

        filter.doFilter(request("/api/tournaments", "1.1.1.1"), FakeResponse().response, chain)
        val secondIp = FakeResponse()
        filter.doFilter(request("/api/tournaments", "2.2.2.2"), secondIp.response, chain)

        verify(chain, times(2)).doFilter(any(), any())
        verify(secondIp.response, never()).status = 429
    }

    @Test
    fun `non-api paths are never rate limited`() {
        val filter = newFilter(capacity = 1)
        val chain = mock(FilterChain::class.java)

        repeat(5) {
            filter.doFilter(request("/actuator/health", "3.3.3.3"), FakeResponse().response, chain)
        }

        verify(chain, times(5)).doFilter(any(), any())
    }

    @Test
    fun `the bucket store evicts once it exceeds maxTrackedIps`() {
        // Regression for the unbounded ConcurrentHashMap this cache replaced: every
        // distinct IP used to allocate a Bucket that lived for the JVM's lifetime.
        val filter = newFilter(capacity = 1, maxTrackedIps = 5)
        val chain = mock(FilterChain::class.java)
        val buckets = RateLimitFilter::class.java.getDeclaredField("buckets").apply { isAccessible = true }
            .get(filter) as com.github.benmanes.caffeine.cache.LoadingCache<*, *>

        repeat(50) { i -> filter.doFilter(request("/api/tournaments", "10.0.0.$i"), FakeResponse().response, chain) }
        buckets.cleanUp()

        assertTrue(buckets.estimatedSize() <= 5, "expected eviction to cap the bucket store at 5, got ${buckets.estimatedSize()}")
    }
}
