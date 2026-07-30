package com.umfl.config

import org.springframework.context.annotation.Configuration
import org.springframework.security.config.annotation.method.configuration.EnableMethodSecurity

/**
 * Second, independent authorization layer on top of the URL matchers in
 * [SecurityConfig] / the `!prod` chain in the same file: `@PreAuthorize` on the
 * admin controllers means a future admin endpoint that forgets a URL matcher is
 * still gated, because the annotation travels with the code. Not
 * profile-scoped -- [com.umfl.auth.ManagerAuthorities] grants `ROLE_ADMIN`
 * identically in every profile, so this needs no per-profile wiring.
 */
@Configuration
@EnableMethodSecurity
class MethodSecurityConfig
