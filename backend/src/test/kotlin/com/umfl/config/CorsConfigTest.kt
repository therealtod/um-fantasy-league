package com.umfl.config

import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc
import org.springframework.security.oauth2.jwt.JwtDecoder
import org.springframework.test.context.ActiveProfiles
import org.springframework.test.context.TestPropertySource
import org.springframework.test.context.bean.override.mockito.MockitoBean
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.options

/**
 * Verifies [ProdCorsConfig]: empty by default (UMFL-01's `_redirects` proxy makes
 * cross-origin calls unnecessary), and origin-specific once `app.frontend-origin`
 * is set. Mirrors [SecurityConfigTest]'s prod-profile `MockMvc` setup.
 */
@AutoConfigureMockMvc
@ActiveProfiles("prod")
@TestPropertySource(
    properties = [
        "spring.datasource.url=jdbc:postgresql://localhost:5432/unused",
        "spring.datasource.username=unused",
        "spring.datasource.password=unused",
        "spring.security.oauth2.resourceserver.jwt.jwk-set-uri=https://example.invalid/.well-known/jwks.json",
    ],
)
class CorsConfigWithoutFrontendOriginTest @Autowired constructor(
    private val mockMvc: MockMvc,
) : PostgresIntegrationTest() {

    @MockitoBean
    private lateinit var jwtDecoder: JwtDecoder

    @Test
    fun `no frontend origin configured means no origin is allowed`() {
        mockMvc.options("/api/tournaments") {
            header("Origin", "http://localhost:5173")
            header("Access-Control-Request-Method", "GET")
        }.andExpect {
            header { doesNotExist("Access-Control-Allow-Origin") }
        }
    }
}

@AutoConfigureMockMvc
@ActiveProfiles("prod")
@TestPropertySource(
    properties = [
        "spring.datasource.url=jdbc:postgresql://localhost:5432/unused",
        "spring.datasource.username=unused",
        "spring.datasource.password=unused",
        "spring.security.oauth2.resourceserver.jwt.jwk-set-uri=https://example.invalid/.well-known/jwks.json",
        "app.frontend-origin=https://umfl.pages.dev",
    ],
)
class CorsConfigWithFrontendOriginTest @Autowired constructor(
    private val mockMvc: MockMvc,
) : PostgresIntegrationTest() {

    @MockitoBean
    private lateinit var jwtDecoder: JwtDecoder

    @Test
    fun `the configured frontend origin is allowed`() {
        mockMvc.options("/api/tournaments") {
            header("Origin", "https://umfl.pages.dev")
            header("Access-Control-Request-Method", "GET")
        }.andExpect {
            header { string("Access-Control-Allow-Origin", "https://umfl.pages.dev") }
        }
    }

    @Test
    fun `localhost is still not allowed once a frontend origin is configured`() {
        mockMvc.options("/api/tournaments") {
            header("Origin", "http://localhost:5173")
            header("Access-Control-Request-Method", "GET")
        }.andExpect {
            header { doesNotExist("Access-Control-Allow-Origin") }
        }
    }
}
