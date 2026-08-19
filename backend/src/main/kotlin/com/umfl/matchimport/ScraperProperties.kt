package com.umfl.matchimport

import org.springframework.boot.context.properties.ConfigurationProperties
import org.springframework.stereotype.Component
import java.time.Duration

/**
 * Where the Tabletop League scraper sidecar lives and how long to wait on it.
 *
 * This is the second exception to this codebase's "no configuration properties"
 * rule, and it earns it the same way [com.umfl.ratelimit.RateLimitProperties]
 * does: the address of a sibling service is operational infrastructure config,
 * the same category as `DB_URL` and `SUPABASE_JWKS_URI`, not domain data that
 * could live in a table. A scoring weight belongs in `scoring_coefficient`
 * because an admin retunes it; a hostname belongs here because it changes when
 * the deployment topology changes, and the application cannot read it from a
 * database it has not connected to yet.
 *
 * See the `scraper` block in `application.yml`.
 */
@Component
@ConfigurationProperties(prefix = "scraper")
data class ScraperProperties(
    /**
     * Base URL of the scraper service. The default is a bare `npm run serve`
     * on the same host, so a `bootRun` dev session needs no configuration at
     * all; `docker-compose.yml` points it at the `scraper` service instead.
     */
    val baseUrl: String = "http://localhost:3000",
    /**
     * How long to wait for one scrape. Generous on purpose: the sidecar loads a
     * client-rendered page and waits for network idle, which the scraper's own
     * `politeGoto` caps at 60s, and it may additionally sit behind that
     * service's politeness delay if another import is already running.
     */
    val timeout: Duration = Duration.ofSeconds(90),
    /**
     * How long to wait for the connection itself. Short: either the sidecar is
     * listening or it is not, and "not running" should surface as a 503 the
     * admin can act on rather than a stalled request.
     */
    val connectTimeout: Duration = Duration.ofSeconds(5),
    /**
     * Hosts an import URL may point at. The sidecar enforces this too — this
     * copy fails a bad URL before it costs a network round trip, and keeps the
     * rule visible on the side that takes it from a user.
     */
    val allowedHosts: List<String> = listOf("www.tabletopleague.com"),
)
