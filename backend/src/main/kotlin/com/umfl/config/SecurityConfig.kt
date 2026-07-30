package com.umfl.config

import com.umfl.auth.DevManagerAuthenticationFilter
import com.umfl.auth.SupabaseAuthenticationConverter
import com.umfl.ratelimit.RateLimitFilter
import org.springframework.beans.factory.annotation.Value
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration
import org.springframework.context.annotation.Profile
import org.springframework.http.HttpMethod
import org.springframework.security.config.Customizer
import org.springframework.security.config.annotation.web.builders.HttpSecurity
import org.springframework.security.config.http.SessionCreationPolicy
import org.springframework.security.oauth2.jose.jws.SignatureAlgorithm
import org.springframework.security.oauth2.jwt.JwtDecoder
import org.springframework.security.oauth2.jwt.NimbusJwtDecoder
import org.springframework.security.oauth2.server.resource.web.authentication.BearerTokenAuthenticationFilter
import org.springframework.security.web.SecurityFilterChain
import org.springframework.security.web.access.intercept.AuthorizationFilter

/**
 * Production security: a stateless JWT resource server verifying Supabase-issued
 * access tokens. See [com.umfl.auth.SupabaseManagerProvider] for how the verified
 * token is resolved into a [com.umfl.manager.Manager], and
 * [com.umfl.auth.SupabaseAuthenticationConverter] for where that resolution — and
 * the `ROLE_ADMIN` authority it carries — actually happens.
 */
@Configuration
@Profile("prod")
class SecurityConfig(
    private val supabaseAuthenticationConverter: SupabaseAuthenticationConverter,
    private val problemDetailAccessDeniedHandler: ProblemDetailAccessDeniedHandler,
    private val problemDetailAuthenticationEntryPoint: ProblemDetailAuthenticationEntryPoint,
    private val rateLimitFilter: RateLimitFilter,
) {

    // Spring Boot's autoconfigured decoder (built from the jwk-set-uri property
    // alone) only accepts RS256. Supabase's current "JWT Signing Keys" projects
    // sign with ES256, so that autoconfigured decoder rejects every real token
    // with "no matching key(s) found" even though the kid is present in the
    // JWKS — hence an explicit decoder naming the algorithm actually in use.
    @Bean
    fun jwtDecoder(@Value("\${spring.security.oauth2.resourceserver.jwt.jwk-set-uri}") jwkSetUri: String): JwtDecoder =
        NimbusJwtDecoder.withJwkSetUri(jwkSetUri).jwsAlgorithm(SignatureAlgorithm.ES256).build()

    @Bean
    fun filterChain(http: HttpSecurity): SecurityFilterChain {
        http
            .cors(Customizer.withDefaults())
            .csrf { it.disable() }
            .sessionManagement { it.sessionCreationPolicy(SessionCreationPolicy.STATELESS) }
            .authorizeHttpRequests {
                it.requestMatchers("/actuator/health", "/actuator/info").permitAll()
                // Viewing tournaments, hero pools and standings needs no account — only
                // entering a tournament and drafting a roster does.
                it.requestMatchers(
                    HttpMethod.GET,
                    "/api/tournaments/*/heroes",
                    "/api/tournaments",
                    "/api/tournaments/*",
                    "/api/tournaments/*/standings",
                    "/api/tournaments/*/standings/stream",
                    "/api/tournaments/*/matches",
                ).permitAll()
                // Must precede the general "/api/**" rule below — first match wins.
                it.requestMatchers("/api/admin/**").hasRole("ADMIN")
                it.requestMatchers(HttpMethod.DELETE, "/api/tournaments/*").hasRole("ADMIN")
                it.requestMatchers("/api/**").authenticated()
                it.anyRequest().denyAll()
            }
            .exceptionHandling {
                it.accessDeniedHandler(problemDetailAccessDeniedHandler)
                it.authenticationEntryPoint(problemDetailAuthenticationEntryPoint)
            }
            .oauth2ResourceServer { oauth2 ->
                oauth2.jwt { jwt -> jwt.jwtAuthenticationConverter(supabaseAuthenticationConverter) }
            }
            // Anchored on BearerTokenAuthenticationFilter, not AuthorizationFilter: that
            // filter sits well before AuthorizationFilter in Spring Security's fixed
            // ordering, so anchoring there is the only way to run ahead of JWT
            // verification itself rather than merely ahead of the authorization decision.
            .addFilterBefore(rateLimitFilter, BearerTokenAuthenticationFilter::class.java)
        return http.build()
    }
}

/**
 * Adding `spring-boot-starter-oauth2-resource-server` pulls Spring Security onto
 * the classpath, whose autoconfiguration secures every endpoint by default
 * (generated user + HTTP Basic) unless a [SecurityFilterChain] bean is present.
 * [SecurityConfig] above supplies that bean only for `prod`; this permissive
 * chain is the explicit equivalent of "no security" for every other profile,
 * except for admin routes, which stay role-gated even in dev/test so the
 * admin API is actually testable without mocking a JWT — see
 * [DevManagerAuthenticationFilter], which resolves the same `X-Manager-Id`
 * header [com.umfl.auth.DevManagerProvider] always read, but does it once at
 * the filter level so `hasRole("ADMIN")` has an authority to check.
 */
@Configuration
@Profile("!prod")
class DevSecurityConfig(
    private val devManagerAuthenticationFilter: DevManagerAuthenticationFilter,
    private val problemDetailAccessDeniedHandler: ProblemDetailAccessDeniedHandler,
    private val rateLimitFilter: RateLimitFilter,
) {

    @Bean
    fun permissiveFilterChain(http: HttpSecurity): SecurityFilterChain {
        http
            .csrf { it.disable() }
            .authorizeHttpRequests {
                it.requestMatchers("/api/admin/**").hasRole("ADMIN")
                it.requestMatchers(HttpMethod.DELETE, "/api/tournaments/*").hasRole("ADMIN")
                it.anyRequest().permitAll()
            }
            .exceptionHandling { it.accessDeniedHandler(problemDetailAccessDeniedHandler) }
            // rateLimitFilter added first so it also runs ahead of the manager lookup
            // devManagerAuthenticationFilter does — both anchored on AuthorizationFilter,
            // and filters sharing an anchor run in the order they were added.
            .addFilterBefore(rateLimitFilter, AuthorizationFilter::class.java)
            .addFilterBefore(devManagerAuthenticationFilter, AuthorizationFilter::class.java)
        return http.build()
    }
}
