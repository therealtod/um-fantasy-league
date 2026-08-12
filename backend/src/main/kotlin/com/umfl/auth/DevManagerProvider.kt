package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.context.annotation.Profile
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component

/**
 * Development stand-in for real authentication.
 *
 * Reads the [Manager] that [DevManagerAuthenticationFilter] already resolved
 * from the `X-Manager-Id` header (or, absent that, some admin manager) and set
 * on the security context — one resolution path, so this and any
 * `hasRole("ADMIN")` route matcher can never disagree about who the request
 * is. Active for every profile except `prod`, where [SupabaseManagerProvider]
 * takes over.
 *
 * NOT SUITABLE FOR ANY DEPLOYED ENVIRONMENT: it trusts a client-supplied header.
 */
@Component
@Profile("!prod")
class DevManagerProvider : CurrentManagerProvider {

    override fun current(): Manager =
        (SecurityContextHolder.getContext().authentication as? ManagerAuthenticationToken)?.principal
            ?: error("No authenticated manager for this request")

    companion object {
        const val MANAGER_ID_HEADER = "X-Manager-Id"
    }
}
