package com.umfl.config

import org.springframework.beans.factory.annotation.Value
import org.springframework.context.annotation.Configuration
import org.springframework.context.annotation.Profile
import org.springframework.web.servlet.config.annotation.CorsRegistry
import org.springframework.web.servlet.config.annotation.WebMvcConfigurer

/**
 * The Vite dev server runs on a different origin during local development.
 * 5273/5274 are the two dev-identity ports the Playwright e2e suite starts
 * (frontend/playwright.config.ts) — one per manager, since dev auth resolves
 * one identity per running frontend.
 */
@Configuration
@Profile("!prod")
class DevCorsConfig : WebMvcConfigurer {

    override fun addCorsMappings(registry: CorsRegistry) {
        registry.addMapping("/api/**")
            .allowedOrigins(
                "http://localhost:5173",
                "http://127.0.0.1:5173",
                "http://localhost:5273",
                "http://127.0.0.1:5273",
                "http://localhost:5274",
                "http://127.0.0.1:5274",
            )
            .allowedMethods("GET", "POST", "PUT", "DELETE", "OPTIONS")
            .allowedHeaders("*")
    }
}

/**
 * The deployed frontend reaches the backend through a same-origin proxy
 * (`frontend/public/_redirects`, UMFL-01), so cross-origin CORS is not on the
 * critical path in prod and this allowlist is empty by default.
 * `FRONTEND_ORIGIN` exists only for a hypothetical frontend deployment that
 * calls the API directly instead of through that proxy.
 */
@Configuration
@Profile("prod")
class ProdCorsConfig(
    @param:Value("\${app.frontend-origin:}") private val frontendOrigin: String,
) : WebMvcConfigurer {

    override fun addCorsMappings(registry: CorsRegistry) {
        if (frontendOrigin.isBlank()) return

        registry.addMapping("/api/**")
            .allowedOrigins(frontendOrigin)
            .allowedMethods("GET", "POST", "PUT", "DELETE", "OPTIONS")
            .allowedHeaders("*")
    }
}
