package com.umfl.config

import com.umfl.api.AdminHeroController
import com.umfl.manager.Manager
import com.umfl.support.PostgresIntegrationTest
import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Test
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.security.access.AccessDeniedException
import org.springframework.security.authentication.TestingAuthenticationToken
import org.springframework.security.core.context.SecurityContextHolder
import kotlin.test.assertFailsWith

/**
 * Proves the `@PreAuthorize` layer on admin controllers rejects a non-admin
 * caller by itself. Calls the Spring-managed [AdminHeroController] bean's
 * method directly -- bypassing MockMvc, the DispatcherServlet, and every URL
 * matcher in SecurityConfig -- so only the method-security AOP proxy can be
 * what denies the call.
 */
class MethodSecurityTest @Autowired constructor(
    private val adminHeroController: AdminHeroController,
) : PostgresIntegrationTest() {

    private val manager = Manager(
        handle = "irrelevant",
        displayName = "irrelevant",
    )

    @AfterEach
    fun clearSecurityContext() {
        SecurityContextHolder.clearContext()
    }

    @Test
    fun `a caller without ROLE_ADMIN is rejected by the method-level annotation alone`() {
        SecurityContextHolder.getContext().authentication = TestingAuthenticationToken("caller", null)

        assertFailsWith<AccessDeniedException> { adminHeroController.list(manager) }
    }

    @Test
    fun `a caller with ROLE_ADMIN passes the method-level annotation`() {
        SecurityContextHolder.getContext().authentication =
            TestingAuthenticationToken("admin", null, "ROLE_ADMIN")

        adminHeroController.list(manager)
    }
}
