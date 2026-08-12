package com.umfl.auth

import com.umfl.common.NotFoundException
import com.umfl.manager.ManagerRepository
import jakarta.servlet.FilterChain
import jakarta.servlet.http.HttpServletRequest
import jakarta.servlet.http.HttpServletResponse
import org.springframework.context.annotation.Profile
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component
import org.springframework.web.filter.OncePerRequestFilter

/**
 * Dev/test equivalent of [SupabaseAuthenticationConverter]: resolves the same
 * `X-Manager-Id` header [DevManagerProvider] always read, but does it once at
 * the filter level so `hasRole("ADMIN")` route matchers work in every
 * non-prod profile too — the whole point of this class. Without it, dev's
 * permissive security chain has no Spring Security principal at all, so an
 * admin-only route could never be matcher-gated the same way it is in prod.
 *
 * Omitting the header impersonates *some* admin manager
 * ([ManagerRepository.findFirstByIsAdminTrueOrderById]) rather than a
 * hardcoded handle, so this filter carries no opinion about what dev/test
 * data seeds — it only needs one manager flagged admin, if any exist at all.
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
        val headerId = request.getHeader(DevManagerProvider.MANAGER_ID_HEADER)?.toLongOrNull()
        val manager = if (headerId != null) {
            managerRepository.findById(headerId).orElseThrow {
                NotFoundException("No manager with id $headerId")
            }
        } else {
            managerRepository.findFirstByIsAdminTrueOrderById()
                ?: throw NotFoundException(
                    "No admin manager to fall back to — pass X-Manager-Id, or seed a manager with is_admin = true",
                )
        }
        SecurityContextHolder.getContext().authentication =
            ManagerAuthenticationToken(manager, null, ManagerAuthorities.of(manager))
        filterChain.doFilter(request, response)
    }
}
