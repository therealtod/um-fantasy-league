package com.umfl.auth

import com.umfl.manager.ManagerRepository
import jakarta.servlet.FilterChain
import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.springframework.context.annotation.Profile
import org.springframework.security.authentication.BadCredentialsException
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component
import org.springframework.web.filter.OncePerRequestFilter

/**
 * Dev/test equivalent of [SupabaseAuthenticationConverter]: resolves the same
 * `X-Manager-Id` header [DevManagerProvider] reads, but does it once at the
 * filter level so `hasRole("ADMIN")` route matchers work in every non-prod
 * profile too — the whole point of this class. Without it, dev's chain has no
 * Spring Security principal at all, so an admin-only route could never be
 * matcher-gated the same way it is in prod.
 *
 * Resolution is conditional on a credential being *offered*, never on the
 * route, exactly like prod: there, [SupabaseAuthenticationConverter] runs only
 * because `BearerTokenAuthenticationFilter` found a bearer token, so an
 * anonymous request to a public route costs no database query and no
 * authentication at all. A request with no `X-Manager-Id` is that same
 * anonymous request here — it passes straight through with an empty security
 * context, and [com.umfl.config.DevSecurityConfig]'s authorization rules (the
 * same ones prod uses) decide whether the route actually needed an identity.
 * This filter deliberately knows nothing about which routes those are.
 *
 * A header that is present but unusable is a different case from an absent
 * one: a credential was offered and it is bad, so it is rejected rather than
 * silently downgraded to anonymous — the same way prod treats a malformed
 * bearer token.
 *
 * Both rejections throw a Spring Security [BadCredentialsException] rather
 * than a domain [com.umfl.common.NotFoundException]: this filter runs ahead of
 * `DispatcherServlet`, so [com.umfl.common.GlobalExceptionHandler] (a
 * `@RestControllerAdvice`) never sees an exception thrown here — only
 * `ExceptionTranslationFilter` does, and only for a genuine
 * `AuthenticationException`. [DevSecurityConfig][com.umfl.config.DevSecurityConfig]
 * wires the same `ProblemDetailAuthenticationEntryPoint` prod uses to catch
 * it, so an unresolvable identity is a clean RFC 7807 401 instead of an
 * unhandled exception surfacing as a bare 500.
 */
@Component
@Profile("!prod")
class DevManagerAuthenticationFilter(
    private val managerRepository: ManagerRepository,
) : OncePerRequestFilter() {

    override fun doFilterInternal(
        request: HttpServletRequest,
        response: HttpServletResponse,
        filterChain: FilterChain,
    ) {
        val header = request.getHeader(DevManagerProvider.MANAGER_ID_HEADER)
        if (header == null) {
            filterChain.doFilter(request, response)
            return
        }

        val managerId = header.toLongOrNull()
            ?: throw BadCredentialsException("Malformed ${DevManagerProvider.MANAGER_ID_HEADER}: $header")
        val manager = managerRepository.findById(managerId).orElseThrow {
            BadCredentialsException("No manager with id $managerId")
        }
        SecurityContextHolder.getContext().authentication =
            ManagerAuthenticationToken(manager, null, ManagerAuthorities.of(manager))
        filterChain.doFilter(request, response)
    }
}
