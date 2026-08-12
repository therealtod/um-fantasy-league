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
 * [com.umfl.auth.DevManagerProvider] reads, once, at the filter level. See
 * [DevSecurityConfigTest] for the route-level half of the dev chain.
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
    fun `omitting the header is anonymous, so an admin route is a 401`() {
        createHero(null, "Dev Anonymous Hero").andExpect {
            status { isUnauthorized() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }

    @Test
    fun `an id naming no manager at all is a clean 401`() {
        createHero("999999", "Dev Unknown Manager Hero").andExpect {
            status { isUnauthorized() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }

    @Test
    fun `a malformed id is rejected rather than treated as anonymous`() {
        createHero("not-a-number", "Dev Malformed Header Hero").andExpect {
            status { isUnauthorized() }
            content { contentType(MediaType.APPLICATION_PROBLEM_JSON) }
        }
    }
}
