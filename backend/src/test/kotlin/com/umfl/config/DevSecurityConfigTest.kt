package com.umfl.config

import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.mockito.ArgumentMatchers.anyLong
import org.mockito.Mockito.never
import org.mockito.Mockito.verify
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc
import org.springframework.http.MediaType
import org.springframework.test.context.bean.override.mockito.MockitoSpyBean
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.get
import org.springframework.test.web.servlet.post

/**
 * The dev-profile twin of [SecurityConfigTest]: both chains enforce the same
 * `apiAuthorizationRules`, so the same routes are public and the same routes
 * demand an identity — only the credential differs (`X-Manager-Id` here, a
 * Supabase JWT there).
 *
 * The lookup assertion below is the point of the whole arrangement.
 * [com.umfl.auth.DevManagerAuthenticationFilter] used to resolve a manager on
 * every request, ahead of `AuthorizationFilter` and therefore ahead of any
 * knowledge of whether the route needed one — which cost a query per public
 * GET and, on a database migrated without `db/seed`, threw before the
 * `permitAll()` rule was ever consulted, turning every public route into a 401.
 */
@AutoConfigureMockMvc
class DevSecurityConfigTest @Autowired constructor(
    private val mockMvc: MockMvc,
) : PostgresIntegrationTest() {

    @MockitoSpyBean
    private lateinit var managerRepository: ManagerRepository

    @Test
    fun `tournament listing is viewable without a manager id`() {
        mockMvc.get("/api/tournaments").andExpect { status { isOk() } }
    }

    @Test
    fun `a public route costs no manager lookup`() {
        mockMvc.get("/api/tournaments").andExpect { status { isOk() } }

        verify(managerRepository, never()).findById(anyLong())
    }

    @Test
    fun `the standings SSE stream is subscribable without a manager id`() {
        // Same caveat as SecurityConfigTest's copy: a live SSE connection never
        // completes, so this only asserts security let it into the controller.
        mockMvc.get("/api/tournaments/1/standings/stream").andExpect { request { asyncStarted() } }
    }

    @Test
    fun `health endpoint is public`() {
        mockMvc.get("/actuator/health").andExpect { status { isOk() } }
    }

    @Test
    fun `a manager route without a manager id is a 401, not an anonymous 500`() {
        mockMvc.get("/api/me").andExpect {
            status { isUnauthorized() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }

    @Test
    fun `entering a tournament without a manager id is a 401`() {
        mockMvc.post("/api/tournaments/1/entries").andExpect { status { isUnauthorized() } }
    }
}
