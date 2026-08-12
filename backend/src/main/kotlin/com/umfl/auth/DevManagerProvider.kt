package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.context.annotation.Profile
import org.springframework.security.core.context.SecurityContextHolder
import org.springframework.stereotype.Component

/**
 * Development stand-in for real authentication.
 *
 * Reads the [Manager] that [DevManagerAuthenticationFilter] already resolved
 * from the `X-Manager-Id` header and set on the security context — one
 * resolution path, so this and any `hasRole("ADMIN")` route matcher can never
 * disagree about who the request is. A request that carried no header is
 * anonymous, and [currentOrNull] returns null for it exactly as
 * [SupabaseManagerProvider] does for a request with no bearer token. Active
 * for every profile except `prod`, where that provider takes over.
 *
 * NOT SUITABLE FOR ANY DEPLOYED ENVIRONMENT: it trusts a client-supplied header.
 */
@Component
@Profile("!prod")
class DevManagerProvider : CurrentManagerProvider {

    override fun current(): Manager =
        currentOrNull() ?: error("No authenticated manager for this request")

    override fun currentOrNull(): Manager? =
        (SecurityContextHolder.getContext().authentication as? ManagerAuthenticationToken)?.principal

    companion object {
        const val MANAGER_ID_HEADER = "X-Manager-Id"
    }
}
