package com.umfl.config

import com.umfl.manager.ManagerRepository
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.Test
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
    fun `omitting the header falls back to NeonStrategist, which is seeded as admin`() {
        createHero(null, "Dev Default Hero").andExpect { status { isCreated() } }
    }
}
