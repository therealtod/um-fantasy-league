package com.umfl.auth

import com.umfl.manager.Manager
import org.springframework.security.core.GrantedAuthority
import org.springframework.security.core.authority.SimpleGrantedAuthority

/**
 * The only place [Manager.isAdmin] turns into a Spring Security authority.
 *
 * Zero knowledge of JWTs, Supabase, or HTTP headers — whatever resolves a
 * [Manager] for the current request (a JWT converter today, a header filter in
 * dev/test, anything else later) hands it here to decide the roles. Swapping
 * the identity provider never touches this.
 */
object ManagerAuthorities {
    fun of(manager: Manager): List<GrantedAuthority> =
        if (manager.isAdmin) listOf(SimpleGrantedAuthority("ROLE_ADMIN")) else emptyList()
}
