package com.umfl.common

import jakarta.servlet.http.HttpServletResponse
import org.slf4j.LoggerFactory
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.http.HttpStatus
import org.springframework.http.ProblemDetail
import org.springframework.web.bind.MethodArgumentNotValidException
import org.springframework.web.bind.annotation.ExceptionHandler
import org.springframework.web.bind.annotation.RestControllerAdvice
import org.springframework.web.context.request.async.AsyncRequestNotUsableException
import java.net.URI

@RestControllerAdvice
class GlobalExceptionHandler {

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

    @ExceptionHandler(MethodArgumentNotValidException::class)
    fun handleValidation(ex: MethodArgumentNotValidException): ProblemDetail =
        problem(
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

    /**
     * Fires when Tomcat's own AsyncContext notices a disconnected client on an SSE stream (see
     * [com.umfl.standings.StandingsSseHub]) and dispatches the error back through MVC on a servlet
     * thread — independently of, and in addition to, the IOException StandingsSseHub already catches
     * and cleans up after when a keep-alive write fails. A closed browser tab mid-stream is routine,
     * not a bug, so log at DEBUG rather than letting it fall through to the WARN-level catch-all below.
     */
    @ExceptionHandler(AsyncRequestNotUsableException::class)
    fun handleAsyncRequestNotUsable(ex: AsyncRequestNotUsableException): ProblemDetail? {
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

    private fun problem(status: HttpStatus, title: String, detail: String?, slug: String) =
        ProblemDetail.forStatus(status).apply {
            this.title = title
            this.detail = detail
            this.type = URI.create("https://umfl.dev/problems/$slug")
        }
}
