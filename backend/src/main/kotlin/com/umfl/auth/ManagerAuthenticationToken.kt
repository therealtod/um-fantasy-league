package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.security.authentication.AbstractAuthenticationToken
import org.springframework.security.core.GrantedAuthority

/**
 * The authentication set on [org.springframework.security.core.context.SecurityContextHolder]
 * once a request has been resolved to a [Manager] — by [SupabaseAuthenticationConverter] in
 * prod, or [DevManagerAuthenticationFilter] everywhere else.
 *
 * `hasRole("ADMIN")` route matchers and `@CurrentManager` both read the same
 * resolved principal, so they can never disagree about who the request is.
 */
class ManagerAuthenticationToken(
    private val manager: Manager,
    /** The raw credential that produced this manager — a JWT in prod, null in dev. Kept for auditability only. */
    private val credentialsValue: Any? = null,
    authorities: Collection<GrantedAuthority>,
) : AbstractAuthenticationToken(authorities) {

    init {
        super.setAuthenticated(true)
    }

    override fun getCredentials(): Any? = credentialsValue
    override fun getPrincipal(): Manager = manager
    override fun getName(): String = manager.handle
}
