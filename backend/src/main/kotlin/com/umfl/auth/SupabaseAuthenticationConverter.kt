package com.umfl.auth

import com.umfl.manager.Manager
import com.umfl.manager.ManagerRepository
import org.springframework.context.annotation.Profile
import org.springframework.core.convert.converter.Converter
import org.springframework.dao.DuplicateKeyException
import org.springframework.security.authentication.AbstractAuthenticationToken
import org.springframework.security.authentication.BadCredentialsException
import org.springframework.security.oauth2.jwt.Jwt
import org.springframework.stereotype.Component
import java.util.UUID

/**
 * Production auth: resolves the manager from a verified Supabase-issued JWT,
 * once per request, at the security-filter level — not inside a controller
 * argument resolver — so route matchers like `hasRole("ADMIN")` see the same
 * resolved identity [CurrentManagerArgumentResolver] later injects.
 *
 * Supabase Auth mints this token regardless of upstream identity provider —
 * Discord OAuth is just how the end user authenticated with Supabase. The
 * `sub` claim is the stable `auth.users.id` UUID.
 *
 * Just-in-time provisioning: the first request from a brand-new Supabase
 * identity has no linked [Manager] row yet, so one is created here, deriving
 * handle/display name from the Discord profile claims Supabase surfaces in
 * `user_metadata`. There is no starting balance to hand out — budget is
 * granted per tournament registration.
 *
 * Authorities come entirely from [ManagerAuthorities], which knows nothing
 * about JWTs — this class's only job is turning a token into a [Manager].
 */
@Component
@Profile("prod")
class SupabaseAuthenticationConverter(
    private val managerRepository: ManagerRepository,
) : Converter<Jwt, AbstractAuthenticationToken> {

    override fun convert(jwt: Jwt): AbstractAuthenticationToken {
        val authUserId = try {
            UUID.fromString(jwt.subject)
        } catch (ex: IllegalArgumentException) {
            throw BadCredentialsException("Malformed sub claim: ${jwt.subject}")
        } catch (ex: NullPointerException) {
            throw BadCredentialsException("Missing sub claim")
        }
        val manager = managerRepository.findByAuthUserId(authUserId) ?: provisionManager(jwt, authUserId)
        return ManagerAuthenticationToken(manager, jwt, ManagerAuthorities.of(manager))
    }

    /**
     * `findByAuthUserId` + `save` below is a check-then-insert with no locking, so two
     * concurrent first requests from the same brand-new identity (e.g. the frontend firing
     * several authenticated calls right after login) can both miss the row and both attempt
     * to provision. One insert wins; the other must recover rather than surface a 500/401.
     * A [DuplicateKeyException] there means either that same race (re-querying finds the row
     * the other request just committed) or two different new users whose derived handles
     * collided (re-querying by authUserId still finds nothing, so retry with a freshly
     * resolved unique handle).
     */
    private fun provisionManager(jwt: Jwt, authUserId: UUID): Manager {
        val metadata = jwt.claims["user_metadata"] as? Map<*, *> ?: emptyMap<String, Any>()
        val discordUsername = metadata["user_name"] as? String
            ?: metadata["full_name"] as? String
            ?: "Manager-${authUserId.toString().take(8)}"

        repeat(MAX_PROVISION_ATTEMPTS) {
            val provisioned = Manager(
                handle = uniqueHandle(discordUsername),
                displayName = discordUsername,
                authUserId = authUserId,
            )
            try {
                return managerRepository.save(provisioned)
            } catch (ex: DuplicateKeyException) {
                managerRepository.findByAuthUserId(authUserId)?.let { return it }
            }
        }
        throw IllegalStateException("Unable to provision manager for auth user $authUserId")
    }

    private companion object {
        const val MAX_PROVISION_ATTEMPTS = 3
    }

    private fun uniqueHandle(base: String): String {
        val sanitized = base.filter { it.isLetterOrDigit() }.ifBlank { "Manager" }
        var candidate = sanitized
        var suffix = 1
        while (managerRepository.findByHandle(candidate) != null) {
            candidate = "$sanitized$suffix"
            suffix++
        }
        return candidate
    }
}
