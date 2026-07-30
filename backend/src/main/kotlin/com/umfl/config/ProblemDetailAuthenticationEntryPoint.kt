package com.umfl.config

import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.slf4j.LoggerFactory
import org.springframework.http.HttpHeaders
import org.springframework.http.HttpStatus
import org.springframework.http.MediaType
import org.springframework.http.ProblemDetail
import org.springframework.security.core.AuthenticationException
import org.springframework.security.web.AuthenticationEntryPoint
import org.springframework.stereotype.Component
import tools.jackson.databind.json.JsonMapper
import java.net.URI

/**
 * Counterpart to [ProblemDetailAccessDeniedHandler] for the unauthenticated
 * case: without this, an unauthenticated request to a protected route hits
 * the resource server's default `BearerTokenAuthenticationEntryPoint`, which
 * returns a bare 401 with no body — the one response shape that would break
 * the RFC 7807 contract every other error on this API follows.
 */
@Component
class ProblemDetailAuthenticationEntryPoint(private val jsonMapper: JsonMapper) : AuthenticationEntryPoint {

    private val log = LoggerFactory.getLogger(javaClass)

    override fun commence(request: HttpServletRequest, response: HttpServletResponse, authException: AuthenticationException) {
        // The response body deliberately stays generic (matching Bearer's
        // convention of not handing token-validation specifics to the
        // client), but that means this is the only place the real reason
        // (expired token, JWKS mismatch, malformed signature, ...) is ever
        // observable — without this it's a silent 401 with no trace anywhere.
        log.warn("Rejected bearer token on {} {}: {}", request.method, request.requestURI, authException.message)
        response.status = HttpServletResponse.SC_UNAUTHORIZED
        response.contentType = MediaType.APPLICATION_PROBLEM_JSON_VALUE
        response.setHeader(HttpHeaders.WWW_AUTHENTICATE, "Bearer")
        val problem = ProblemDetail.forStatus(HttpStatus.UNAUTHORIZED).apply {
            title = "Unauthorized"
            detail = "Authentication is required to access this resource."
            type = URI.create("https://umfl.dev/problems/unauthorized")
        }
        jsonMapper.writeValue(response.writer, problem)
    }
}
