package com.umfl.config

import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.mockito.Mockito.`when`
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc
import org.springframework.http.MediaType
import org.springframework.security.oauth2.jwt.Jwt
import org.springframework.security.oauth2.jwt.JwtClaimNames
import org.springframework.security.oauth2.jwt.JwtDecoder
import org.springframework.test.context.ActiveProfiles
import org.springframework.test.context.TestPropertySource
import org.springframework.test.context.bean.override.mockito.MockitoBean
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.get
import org.springframework.test.web.servlet.post
import java.time.Instant
import java.util.UUID

/**
 * Verifies the `prod`-only [SecurityConfig] filter chain: API routes require a
 * verified JWT, health stays public. The JWT decoder is mocked out so no real
 * network call to a JWKS endpoint happens in tests; the datasource still comes
 * from [PostgresIntegrationTest]'s Testcontainers Postgres, which takes
 * precedence over the dummy `spring.datasource` values below regardless.
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
class SecurityConfigTest @Autowired constructor(
    private val mockMvc: MockMvc,
    private val managerRepository: ManagerRepository,
) : PostgresIntegrationTest() {

    @MockitoBean
    private lateinit var jwtDecoder: JwtDecoder

    @Test
    fun `protected endpoint rejects requests without a token`() {
        mockMvc.get("/api/me").andExpect { status { isUnauthorized() } }
    }

    @Test
    fun `health endpoint is public`() {
        mockMvc.get("/actuator/health").andExpect { status { isOk() } }
    }

    @Test
    fun `tournament listing is viewable without a token`() {
        mockMvc.get("/api/tournaments").andExpect { status { isOk() } }
    }

    @Test
    fun `a tournament's hero pool is viewable without a token`() {
        mockMvc.get("/api/tournaments/1/heroes").andExpect { status { isOk() } }
    }

    @Test
    fun `the standings SSE stream is subscribable without a token`() {
        // A live SSE connection never completes on its own, so this only asserts
        // that security let the request through into the (async) controller
        // method rather than rejecting it with 401 before it got there.
        mockMvc.get("/api/tournaments/1/standings/stream").andExpect { request { asyncStarted() } }
    }

    @Test
    fun `entering a tournament still requires a token`() {
        mockMvc.post("/api/tournaments/1/entries").andExpect { status { isUnauthorized() } }
    }

    @Test
    fun `admin route rejects requests without a token`() {
        mockMvc.post("/api/admin/heroes").andExpect { status { isUnauthorized() } }
    }

    @Test
    fun `unauthenticated rejection is shaped as a problem detail`() {
        mockMvc.post("/api/admin/heroes")
            .andExpect {
                status { isUnauthorized() }
                content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
                jsonPath("$.title") { value("Unauthorized") }
            }
    }

    @Test
    fun `admin route rejects a token belonging to a non-admin manager`() {
        val authUserId = UUID.randomUUID()
        `when`(jwtDecoder.decode("a-token")).thenReturn(jwt(authUserId))

        mockMvc.post("/api/admin/heroes") {
            header("Authorization", "Bearer a-token")
            contentType = MediaType.APPLICATION_JSON
            content = """{"name": "Test Hero"}"""
        }.andExpect { status { isForbidden() } }
    }

    @Test
    fun `admin route accepts a token belonging to an admin manager`() {
        val authUserId = UUID.randomUUID()
        val admin = requireNotNull(managerRepository.findByHandle("NeonStrategist"))
        managerRepository.save(admin.copy(authUserId = authUserId))
        `when`(jwtDecoder.decode("admin-token")).thenReturn(jwt(authUserId))

        mockMvc.post("/api/admin/heroes") {
            header("Authorization", "Bearer admin-token")
            contentType = MediaType.APPLICATION_JSON
            content = """{"name": "Brand New Hero"}"""
        }.andExpect { status { isCreated() } }
    }

    private fun jwt(subject: UUID): Jwt =
        Jwt.withTokenValue("token")
            .header("alg", "RS256")
            .claim(JwtClaimNames.SUB, subject.toString())
            .issuedAt(Instant.now())
            .expiresAt(Instant.now().plusSeconds(3600))
            .build()
}
