package com.umfl.common

import jakarta.servlet.http.HttpServletResponse
import org.slf4j.LoggerFactory
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.http.HttpHeaders
import org.springframework.http.HttpStatus
import org.springframework.http.HttpStatusCode
import org.springframework.http.ProblemDetail
import org.springframework.http.ResponseEntity
import org.springframework.security.access.AccessDeniedException
import org.springframework.security.core.AuthenticationException
import org.springframework.web.bind.MethodArgumentNotValidException
import org.springframework.web.bind.annotation.ExceptionHandler
import org.springframework.web.bind.annotation.RestControllerAdvice
import org.springframework.web.context.request.WebRequest
import org.springframework.web.context.request.async.AsyncRequestNotUsableException
import org.springframework.web.servlet.mvc.method.annotation.ResponseEntityExceptionHandler
import java.net.URI

/**
 * Every error this API returns, as an RFC 7807 problem detail.
 *
 * Extending [ResponseEntityExceptionHandler] is load-bearing, not decoration.
 * `ExceptionHandlerExceptionResolver` runs *ahead of*
 * `DefaultHandlerExceptionResolver`, so an `@ExceptionHandler(Exception)` in an
 * advice intercepts Spring MVC's own exceptions before the resolver that knows
 * their real status ever sees them — a path variable that won't parse, a
 * malformed JSON body, a wrong HTTP method all rendered as 500. Inheriting the
 * base class's mappings puts those back at 400/405/415/404 while
 * [handleUnexpected] stays the last resort for what is genuinely unexpected.
 *
 * That inheritance also constrains this class: [ResponseEntityExceptionHandler]
 * already carries an `@ExceptionHandler` for every exception listed on its
 * final `handleException`, and two methods mapping the same exception type is
 * an ambiguous mapping that fails context startup. Anything in that set is
 * therefore customised by **overriding** the matching `protected` hook (see
 * [handleMethodArgumentNotValid], [handleAsyncRequestNotUsableException]), never
 * by declaring a second `@ExceptionHandler` for it.
 */
@RestControllerAdvice
class GlobalExceptionHandler : ResponseEntityExceptionHandler() {

    private val log = LoggerFactory.getLogger(javaClass)

    @ExceptionHandler(NotFoundException::class)
    fun handleNotFound(ex: NotFoundException): ProblemDetail =
        problem(HttpStatus.NOT_FOUND, "Not found", ex.message, "not-found")

    @ExceptionHandler(ConflictException::class)
    fun handleConflict(ex: ConflictException): ProblemDetail =
        problem(HttpStatus.CONFLICT, "Conflict", ex.message, "conflict")

    @ExceptionHandler(ServiceUnavailableException::class)
    fun handleServiceUnavailable(ex: ServiceUnavailableException): ProblemDetail =
        problem(HttpStatus.SERVICE_UNAVAILABLE, "Service unavailable", ex.message, "service-unavailable")

    @ExceptionHandler(RosterRuleException::class)
    fun handleRosterRule(ex: RosterRuleException): ProblemDetail =
        problem(
            HttpStatus.UNPROCESSABLE_ENTITY,
            "Roster rules violated",
            ex.message,
            "roster-rule-violation",
        ).apply {
            setProperty(
                "violations",
                ex.violations.map { mapOf("rule" to it.rule.name, "message" to it.message) },
            )
        }

    @ExceptionHandler(MatchRuleException::class)
    fun handleMatchRule(ex: MatchRuleException): ProblemDetail =
        problem(
            HttpStatus.UNPROCESSABLE_ENTITY,
            "Match result rules violated",
            ex.message,
            "match-rule-violation",
        ).apply {
            setProperty(
                "violations",
                ex.violations.map { mapOf("rule" to it.rule.name, "message" to it.message) },
            )
        }

    @ExceptionHandler(ScoringRuleException::class)
    fun handleScoringRule(ex: ScoringRuleException): ProblemDetail =
        problem(
            HttpStatus.UNPROCESSABLE_ENTITY,
            "Scoring rule set rules violated",
            ex.message,
            "scoring-rule-violation",
        ).apply {
            setProperty(
                "violations",
                ex.violations.map { mapOf("rule" to it.rule.name, "message" to it.message) },
            )
        }

    /**
     * The `@PreAuthorize` layer on the admin controllers, rendered the same way
     * the URL-matcher layer renders itself.
     *
     * Method security throws this from *inside* the dispatch, so it is resolved
     * here rather than by `ExceptionTranslationFilter` — without this handler it
     * fell through to [handleUnexpected] and a denial surfaced as a 500 logged as
     * "Unhandled exception". Body and status are kept identical to
     * `ProblemDetailAccessDeniedHandler`'s, so which of the two layers denied a
     * request is invisible to the client, as it should be.
     */
    @ExceptionHandler(AccessDeniedException::class)
    fun handleAccessDenied(ex: AccessDeniedException): ProblemDetail =
        problem(
            HttpStatus.FORBIDDEN,
            "Forbidden",
            "You do not have permission to perform this action.",
            "forbidden",
        )

    /**
     * The unauthenticated counterpart to [handleAccessDenied], mirroring
     * `ProblemDetailAuthenticationEntryPoint` for the same reason: method
     * security raises this when it finds no authentication at all, from inside
     * the dispatch where the entry point cannot reach it.
     *
     * Unreachable in practice — every route under `/api` already requires an
     * identity at the matcher layer, so an anonymous caller never gets as far as
     * a controller method — which is exactly the point: the fallback layer needs
     * to answer correctly on the day the matcher list is the thing that's wrong.
     */
    @ExceptionHandler(AuthenticationException::class)
    fun handleAuthentication(ex: AuthenticationException): ProblemDetail =
        problem(
            HttpStatus.UNAUTHORIZED,
            "Unauthorized",
            "Authentication is required to access this resource.",
            "unauthorized",
        )

    /**
     * Backstop for a foreign key / unique constraint reaching the database
     * unvalidated — every known case is caught earlier by a domain policy
     * (e.g. [com.umfl.match.MatchResultPolicy]), so this should never fire in
     * practice. It exists so an unanticipated one is a 409, not a raw 500.
     */
    @ExceptionHandler(DataIntegrityViolationException::class)
    fun handleDataIntegrityViolation(ex: DataIntegrityViolationException): ProblemDetail {
        log.warn("Data integrity violation reached the API layer unvalidated", ex)
        return problem(
            HttpStatus.CONFLICT,
            "Conflict",
            "The request conflicts with existing data.",
            "data-integrity-violation",
        )
    }

    /**
     * Bean-validation failures on a request body, carrying the per-field detail
     * the plain 400 the base class produces would drop.
     *
     * An override rather than an `@ExceptionHandler` of its own: see the class
     * doc — [ResponseEntityExceptionHandler] already maps this type, and mapping
     * it twice fails context startup.
     */
    override fun handleMethodArgumentNotValid(
        ex: MethodArgumentNotValidException,
        headers: HttpHeaders,
        status: HttpStatusCode,
        request: WebRequest,
    ): ResponseEntity<Any>? {
        val body = problem(
            HttpStatus.BAD_REQUEST,
            "Invalid request",
            "One or more fields failed validation.",
            "validation-failed",
        ).apply {
            setProperty(
                "fields",
                ex.bindingResult.fieldErrors.associate { it.field to (it.defaultMessage ?: "invalid") },
            )
        }
        return handleExceptionInternal(ex, body, headers, status, request)
    }

    /**
     * Fires when Tomcat's own AsyncContext notices a disconnected client on an SSE stream (see
     * [com.umfl.standings.StandingsSseHub]) and dispatches the error back through MVC on a servlet
     * thread — independently of, and in addition to, the IOException StandingsSseHub already catches
     * and cleans up after when a keep-alive write fails. A closed browser tab mid-stream is routine,
     * not a bug, so it is logged at DEBUG and rendered with no body at all, which is also what the
     * base class does with it: there is nothing useful to write to a response the client has gone
     * away from.
     */
    override fun handleAsyncRequestNotUsableException(
        ex: AsyncRequestNotUsableException,
        request: WebRequest,
    ): ResponseEntity<Any>? {
        log.debug("Async request no longer usable (client disconnected)", ex)
        return null
    }

    /**
     * A response can already be committed here — e.g. an SSE stream (see [com.umfl.standings.StandingsSseHub])
     * whose Content-Type is fixed to text/event-stream long before some later async failure (a client
     * disconnect, a race on the emitter's timeout) reaches this handler. Writing a ProblemDetail body at
     * that point fails with `HttpMessageNotWritableException: No converter for ProblemDetail with preset
     * Content-Type 'text/event-stream'` since no converter produces problem+json as event-stream. There is
     * nothing useful to send once the response is committed, so skip rendering a body entirely.
     *
     * Everything Spring MVC raises itself is claimed by [ResponseEntityExceptionHandler] before it can
     * reach this method, so what lands here really is unexpected and really is a 500 — which is what
     * makes the ERROR log below meaningful rather than routine.
     */
    @ExceptionHandler(Exception::class)
    fun handleUnexpected(ex: Exception, response: HttpServletResponse): ProblemDetail? {
        if (response.isCommitted) {
            log.warn("Exception after response already committed; cannot render a problem body", ex)
            return null
        }
        log.error("Unhandled exception", ex)
        return problem(
            HttpStatus.INTERNAL_SERVER_ERROR,
            "Internal server error",
            "An unexpected error occurred.",
            "internal-error",
        )
    }

    /**
     * Gives the base class's own problem bodies the same `type` vocabulary the
     * hand-written ones above carry.
     *
     * Spring builds those bodies from the exception's `ErrorResponse` view,
     * which leaves `type` unset — so without this, "every error names a umfl
     * problem type" would hold for domain errors and quietly stop holding for a
     * malformed request. The slug comes from the status' reason phrase, which
     * lands on the same spelling the explicit slugs already use (404 ->
     * `not-found`, 409 -> `conflict`).
     *
     * Only `type` is filled in: `ProblemDetail.getTitle()` already falls back to
     * the reason phrase on its own, and `detail` is the framework's to write —
     * it says more about the specific failure than a generic line here could.
     */
    override fun handleExceptionInternal(
        ex: Exception,
        body: Any?,
        headers: HttpHeaders,
        statusCode: HttpStatusCode,
        request: WebRequest,
    ): ResponseEntity<Any>? {
        val response = super.handleExceptionInternal(ex, body, headers, statusCode, request)
        val problem = response?.body as? ProblemDetail ?: return response
        if (problem.type == null || problem.type == BLANK_TYPE) {
            val slug = HttpStatus.resolve(statusCode.value())?.reasonPhrase?.slug() ?: "error"
            problem.type = URI.create(PROBLEM_TYPE_PREFIX + slug)
        }
        return response
    }

    private fun problem(status: HttpStatus, title: String, detail: String?, slug: String) =
        ProblemDetail.forStatus(status).apply {
            this.title = title
            this.detail = detail
            this.type = URI.create(PROBLEM_TYPE_PREFIX + slug)
        }

    private companion object {
        const val PROBLEM_TYPE_PREFIX = "https://umfl.dev/problems/"

        /**
         * RFC 7807's "no type of its own" sentinel. Spring leaves `type` null
         * rather than setting this, but a `ProblemDetail` built by hand
         * elsewhere may carry it, and both mean the same thing here.
         */
        val BLANK_TYPE: URI = URI.create("about:blank")

        /** `Method Not Allowed` -> `method-not-allowed`. */
        fun String.slug(): String = lowercase().replace(' ', '-')
    }
}
