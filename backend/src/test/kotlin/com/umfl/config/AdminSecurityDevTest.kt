package com.umfl.config

import com.umfl.common.NotFoundException
import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc
import org.springframework.http.MediaType
import org.springframework.test.web.servlet.MockMvc
import org.springframework.test.web.servlet.post

/**
 * Proves admin routes are role-gated in dev/test too, purely via
 * `X-Manager-Id` — no JWT mocking needed, because
 * [com.umfl.auth.DevManagerAuthenticationFilter] resolves the same header
 * [com.umfl.auth.DevManagerProvider] always read, once, at the filter level.
 */
@AutoConfigureMockMvc
class AdminSecurityDevTest @Autowired constructor(
    private val mockMvc: MockMvc,
    private val managerRepository: ManagerRepository,
) : PostgresIntegrationTest() {

    private fun createHero(managerIdHeader: String?, name: String) =
        mockMvc.post("/api/admin/heroes") {
            managerIdHeader?.let { header("X-Manager-Id", it) }
            contentType = MediaType.APPLICATION_JSON
            content = """{"name": "$name"}"""
        }

    @Test
    fun `an admin manager's id is accepted`() {
        val admin = requireNotNull(managerRepository.findByHandle("NeonStrategist"))
        createHero(admin.id.toString(), "Dev Admin Hero").andExpect { status { isCreated() } }
    }

    @Test
    fun `a non-admin manager's id is rejected with 403`() {
        val nonAdmin = requireNotNull(managerRepository.findByHandle("SherlockMain"))
        createHero(nonAdmin.id.toString(), "Dev Rejected Hero").andExpect { status { isForbidden() } }
    }

    @Test
    fun `omitting the header falls back to the seeded admin manager`() {
        createHero(null, "Dev Default Hero").andExpect { status { isCreated() } }
    }

    @Test
    fun `omitting the header with no admin manager at all fails loudly instead of impersonating a fixed handle`() {
        managerRepository.findAll().forEach {
            managerRepository.save(it.copy(isAdmin = false))
        }

        // Thrown from inside the security filter chain, ahead of DispatcherServlet, so
        // GlobalExceptionHandler never sees it -- MockMvc surfaces it as a raw exception
        // rather than a mapped 404. The behaviour under test is *what* fails (no manager
        // resolved, no assumption about who "the" admin is), not the transport shape of
        // that failure.
        val ex = assertThrows<NotFoundException> {
            createHero(null, "Dev Default Hero")
        }
        assert(ex.message?.contains("No admin manager") == true) { "unexpected message: ${ex.message}" }
    }
}
