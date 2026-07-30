package com.umfl.config

import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.springframework.http.HttpStatus
import org.springframework.http.MediaType
import org.springframework.http.ProblemDetail
import org.springframework.security.access.AccessDeniedException
import org.springframework.security.web.access.AccessDeniedHandler
import org.springframework.stereotype.Component
import tools.jackson.databind.json.JsonMapper
import java.net.URI

/**
 * Every error response on this API is an RFC 7807 [ProblemDetail]
 * ([com.umfl.common.GlobalExceptionHandler]) — Spring Security's default 403
 * has no body at all, which would be the one response shape that breaks that
 * contract, since it's raised by the filter chain before Spring MVC's
 * exception handling ever sees the request.
 */
@Component
class ProblemDetailAccessDeniedHandler(private val jsonMapper: JsonMapper) : AccessDeniedHandler {

    override fun handle(request: HttpServletRequest, response: HttpServletResponse, ex: AccessDeniedException) {
        response.status = HttpServletResponse.SC_FORBIDDEN
        response.contentType = MediaType.APPLICATION_PROBLEM_JSON_VALUE
        val problem = ProblemDetail.forStatus(HttpStatus.FORBIDDEN).apply {
            title = "Forbidden"
            detail = "You do not have permission to perform this action."
            type = URI.create("https://umfl.dev/problems/forbidden")
        }
        jsonMapper.writeValue(response.writer, problem)
    }
}
