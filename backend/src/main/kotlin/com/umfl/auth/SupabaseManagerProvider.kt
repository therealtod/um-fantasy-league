package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.context.annotation.Profile
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component

/**
 * Production auth: reads the [Manager] that [SupabaseAuthenticationConverter]
 * already resolved and set on the security context while authenticating the
 * JWT. There is deliberately no repository lookup here any more — resolving
 * and JIT-provisioning a manager happens exactly once, at the filter level,
 * so this and any `hasRole("ADMIN")` route matcher can never see two
 * different answers for the same request.
 */
@Component
@Profile("prod")
class SupabaseManagerProvider : CurrentManagerProvider {

    override fun current(): Manager =
        currentOrNull() ?: error("No authenticated manager for this request")

    override fun currentOrNull(): Manager? =
        (SecurityContextHolder.getContext().authentication as? ManagerAuthenticationToken)?.principal
}
