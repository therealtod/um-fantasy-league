package com.umfl.config

import com.umfl.auth.DevManagerAuthenticationFilter
import com.umfl.auth.SupabaseAuthenticationConverter
import com.umfl.ratelimit.RateLimitFilter
import org.springframework.beans.factory.annotation.Value
import org.springframework.boot.web.servlet.FilterRegistrationBean
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration
import org.springframework.context.annotation.Profile
import org.springframework.http.HttpMethod
import org.springframework.security.config.Customizer
import org.springframework.security.config.annotation.web.builders.HttpSecurity
import org.springframework.security.config.annotation.web.configurers.AuthorizeHttpRequestsConfigurer
import org.springframework.security.config.http.SessionCreationPolicy
import org.springframework.security.oauth2.jose.jws.SignatureAlgorithm
import org.springframework.security.oauth2.jwt.JwtDecoder
import org.springframework.security.oauth2.jwt.NimbusJwtDecoder
import org.springframework.security.oauth2.server.resource.web.authentication.BearerTokenAuthenticationFilter
import org.springframework.security.web.SecurityFilterChain
import org.springframework.security.web.access.intercept.AuthorizationFilter

/**
 * The one place that says which routes need an identity, shared by both filter
 * chains below so `prod` and every other profile can never disagree about it.
 * The chains differ only in how a credential is *verified* — a Supabase JWT in
 * prod, an `X-Manager-Id` header in dev/test — never in which routes require
 * one, which is also why neither
 * [com.umfl.auth.SupabaseAuthenticationConverter] nor
 * [com.umfl.auth.DevManagerAuthenticationFilter] has any route knowledge of its
 * own. Keep this in step with [com.umfl.config.SecurityConfigTest] and
 * [com.umfl.config.DevSecurityConfigTest], which assert it from either side.
 */
private fun apiAuthorizationRules(
    registry: AuthorizeHttpRequestsConfigurer<HttpSecurity>.AuthorizationManagerRequestMatcherRegistry,
) {
    registry.requestMatchers("/actuator/health", "/actuator/info").permitAll()
    // Viewing tournaments, hero pools and standings needs no account — only
    // entering a tournament and drafting a roster does.
    registry.requestMatchers(
        HttpMethod.GET,
        "/api/tournaments/*/heroes",
        "/api/tournaments",
        "/api/tournaments/*",
        "/api/tournaments/*/standings",
        "/api/tournaments/*/standings/stream",
        "/api/tournaments/*/matches",
    ).permitAll()
    // Must precede the general "/api/**" rule below — first match wins.
    registry.requestMatchers("/api/admin/**").hasRole("ADMIN")
    registry.requestMatchers("/api/**").authenticated()
    registry.anyRequest().denyAll()
}

/**
 * Production security: a stateless JWT resource server verifying Supabase-issued
 * access tokens. See [com.umfl.auth.SecurityContextManagerProvider] for how the
 * verified token is resolved into a [com.umfl.manager.Manager], and
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
            .authorizeHttpRequests(::apiAuthorizationRules)
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

    // rateLimitFilter is a @Component, so Boot's servlet-filter autoconfiguration would
    // otherwise register it a second time as a plain /* filter alongside the copy wired
    // into the security chain above. That second copy is a no-op today — OncePerRequestFilter
    // marks the request as already filtered — but it's an ordering accident, not a guarantee,
    // so disable the auto-registration explicitly and keep only the one addFilterBefore wires in.
    @Bean
    fun rateLimitFilterRegistration(): FilterRegistrationBean<RateLimitFilter> =
        FilterRegistrationBean(rateLimitFilter).apply { isEnabled = false }
}

/**
 * Adding `spring-boot-starter-oauth2-resource-server` pulls Spring Security onto
 * the classpath, whose autoconfiguration secures every endpoint by default
 * (generated user + HTTP Basic) unless a [SecurityFilterChain] bean is present.
 * [SecurityConfig] above supplies that bean only for `prod`; this is that bean
 * for every other profile — and it enforces the very same
 * [apiAuthorizationRules], so dev and prod agree on which routes need an
 * identity and differ only in how a credential is verified. Here that
 * credential is an `X-Manager-Id` header rather than a Supabase JWT, resolved
 * by [DevManagerAuthenticationFilter] once at the filter level so
 * `hasRole("ADMIN")` has an authority to check and the admin API is testable
 * without mocking a JWT.
 */
@Configuration
@Profile("!prod")
class DevSecurityConfig(
    private val devManagerAuthenticationFilter: DevManagerAuthenticationFilter,
    private val problemDetailAccessDeniedHandler: ProblemDetailAccessDeniedHandler,
    private val problemDetailAuthenticationEntryPoint: ProblemDetailAuthenticationEntryPoint,
    private val rateLimitFilter: RateLimitFilter,
) {

    @Bean
    fun devFilterChain(http: HttpSecurity): SecurityFilterChain {
        http
            .csrf { it.disable() }
            .authorizeHttpRequests(::apiAuthorizationRules)
            .exceptionHandling {
                it.accessDeniedHandler(problemDetailAccessDeniedHandler)
                // Reached both by an unresolvable X-Manager-Id (which
                // devManagerAuthenticationFilter throws on) and by an anonymous
                // request to a route apiAuthorizationRules marks authenticated().
                it.authenticationEntryPoint(problemDetailAuthenticationEntryPoint)
            }
            // rateLimitFilter added first so it also runs ahead of the manager lookup
            // devManagerAuthenticationFilter does — both anchored on AuthorizationFilter,
            // and filters sharing an anchor run in the order they were added.
            .addFilterBefore(rateLimitFilter, AuthorizationFilter::class.java)
            .addFilterBefore(devManagerAuthenticationFilter, AuthorizationFilter::class.java)
        return http.build()
    }

    // Both filters are @Component beans, so Boot's servlet-filter autoconfiguration would
    // otherwise register each a second time as a plain /* filter alongside the copies wired
    // into the security chain above. See rateLimitFilterRegistration in SecurityConfig for why
    // that's an ordering accident worth closing rather than relying on — for
    // devManagerAuthenticationFilter in particular, an auto-registered copy running ahead of
    // ExceptionTranslationFilter would let its BadCredentialsException surface as a bare 500
    // instead of the RFC 7807 401 documented on its KDoc.
    @Bean
    fun rateLimitFilterRegistration(): FilterRegistrationBean<RateLimitFilter> =
        FilterRegistrationBean(rateLimitFilter).apply { isEnabled = false }

    @Bean
    fun devManagerAuthenticationFilterRegistration(): FilterRegistrationBean<DevManagerAuthenticationFilter> =
        FilterRegistrationBean(devManagerAuthenticationFilter).apply { isEnabled = false }
}
