package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component

/**
 * Reads the [Manager] that either [DevManagerAuthenticationFilter] (dev/test)
 * or [SupabaseAuthenticationConverter] (prod) already resolved and set on the
 * security context. Both auth paths converge on the same
 * [ManagerAuthenticationToken], so there's exactly one resolution path per
 * profile and no separate provider needed per profile — this and any
 * `hasRole("ADMIN")` route matcher can never disagree about who the request
 * is. A request that carried no credential is anonymous, and [currentOrNull]
 * returns null for it.
 */
@Component
class SecurityContextManagerProvider : CurrentManagerProvider {

    override fun current(): Manager =
        currentOrNull() ?: error("No authenticated manager for this request")

    override fun currentOrNull(): Manager? =
        (SecurityContextHolder.getContext().authentication as? ManagerAuthenticationToken)?.principal
}
