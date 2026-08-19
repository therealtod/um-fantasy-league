package com.umfl.matchimport

import com.umfl.common.ConflictException
import com.umfl.common.ServiceUnavailableException
import org.springframework.http.client.JdkClientHttpRequestFactory
import org.springframework.stereotype.Component
import org.springframework.web.client.ResourceAccessException
import org.springframework.web.client.RestClient
import org.springframework.web.client.RestClientException
import java.net.URI
import java.net.URISyntaxException
import java.net.http.HttpClient

/**
 * Talks to the Tabletop League scraper sidecar
 * (`tools/tabletopleague-scraper/server.mjs`).
 *
 * This is the only outbound HTTP call in the application, and it exists because
 * tabletopleague.com is client-rendered Next.js: fetching a match page from here
 * would return the JS bundle and no data, so a real browser has to render it.
 * That browser is a separate process — hence a sidecar rather than a library.
 *
 * Failure modes are deliberately mapped to two different statuses, because the
 * admin's next action differs: a sidecar that isn't reachable is a 503 ("start
 * it, then retry"), while a sidecar that answered with an error scraped a real
 * page and failed on it — a 409 naming what went wrong, usually selector drift
 * after the source site changed its markup.
 */
@Component
class ScraperClient(private val properties: ScraperProperties) {

    private val restClient: RestClient = RestClient.builder()
        .baseUrl(properties.baseUrl)
        .requestFactory(
            JdkClientHttpRequestFactory(
                HttpClient.newBuilder().connectTimeout(properties.connectTimeout).build(),
            ).apply { setReadTimeout(properties.timeout) },
        )
        .build()

    /**
     * Scrapes one match detail page. [sourceUrl] is expected to have passed
     * [validateSourceUrl] already; the sidecar validates it a second time.
     */
    fun scrapeMatch(sourceUrl: String): ScrapedMatch {
        val response = try {
            restClient.post()
                .uri("/scrape/match")
                .body(mapOf("url" to sourceUrl))
                .retrieve()
                .onStatus({ it.isError }) { _, res ->
                    // The sidecar reports its own failures as {"error": "..."}.
                    // Read it so the admin sees the real reason rather than a status code.
                    val detail = runCatching { res.body.readAllBytes().decodeToString() }.getOrNull()
                    throw ConflictException(
                        "The scraper could not read that match page: ${scraperErrorMessage(detail)}",
                    )
                }
                .body(ScrapedMatch::class.java)
        } catch (ex: ResourceAccessException) {
            // Connection refused, DNS failure, or the read timeout elapsed.
            throw ServiceUnavailableException(
                "The match scraper at ${properties.baseUrl} is not reachable. " +
                    "Start it with `npm run serve` in tools/tabletopleague-scraper, " +
                    "or bring up the `scraper` service. (${ex.mostSpecificCause})",
            )
        } catch (ex: ConflictException) {
            throw ex
        } catch (ex: RestClientException) {
            throw ConflictException("The scraper returned an unusable response: ${ex.message}")
        }

        return response
            ?: throw ConflictException("The scraper returned an empty response for $sourceUrl")
    }

    /**
     * Rejects anything that isn't a match detail page on an allowed host,
     * before it costs a round trip. The sidecar enforces the same rule — this
     * copy is the one that sees a URL typed by a user.
     */
    fun validateSourceUrl(sourceUrl: String): String? {
        // URI(String) throws the *checked* URISyntaxException, not the
        // IllegalArgumentException that URI.create would -- catching the wrong
        // one turns a typo into a 500.
        val uri = try {
            URI(sourceUrl.trim())
        } catch (_: URISyntaxException) {
            return "That is not a valid URL."
        }
        if (uri.scheme != "https") return "The match URL must start with https://."
        val host = uri.host ?: return "That URL has no host."
        if (host !in properties.allowedHosts) {
            return "Only ${properties.allowedHosts.joinToString(", ")} match pages can be imported."
        }
        if (!MATCH_PATH.matches(uri.path ?: "")) {
            return "That is not a match page. Expected a link like " +
                "https://$host/o/<org>/<competition>/matches/<id>."
        }
        return null
    }

    private fun scraperErrorMessage(body: String?): String {
        if (body.isNullOrBlank()) return "no detail given"
        // Cheap extraction rather than a full parse: this is an error path, and
        // the body is the sidecar's own tiny {"error": "..."} object.
        return ERROR_FIELD.find(body)?.groupValues?.get(1) ?: body.take(300)
    }

    private companion object {
        val MATCH_PATH = Regex("""/o/[^/]+/[^/]+/matches/[0-9a-fA-F-]{8,}/?""")
        val ERROR_FIELD = Regex(""""error"\s*:\s*"((?:[^"\\]|\\.)*)"""")
    }
}
