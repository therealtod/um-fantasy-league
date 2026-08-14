package com.umfl.common

import jakarta.validation.Valid
import jakarta.validation.constraints.Positive
import org.junit.jupiter.api.Test
import org.springframework.http.MediaType
import org.springframework.mock.web.MockHttpServletResponse
import org.springframework.security.access.AccessDeniedException
import org.springframework.security.authentication.AuthenticationCredentialsNotFoundException
import org.springframework.test.web.servlet.request.MockMvcRequestBuilders.delete
import org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get
import org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post
import org.springframework.test.web.servlet.setup.MockMvcBuilders
import org.springframework.validation.beanvalidation.LocalValidatorFactoryBean
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RestController
import kotlin.test.assertContains
import kotlin.test.assertEquals

/**
 * Guards the status codes [GlobalExceptionHandler] hands back, through a real
 * `DispatcherServlet` rather than by calling its methods.
 *
 * The defect this exists for is invisible to a direct unit call: an
 * `@ExceptionHandler(Exception)` in an advice is consulted *before*
 * `DefaultHandlerExceptionResolver`, so Spring MVC's own exceptions —
 * an unparseable path variable, a malformed body, a wrong method — were being
 * swallowed by the catch-all and answered as 500. Only the resolver ordering a
 * dispatch actually performs can show that, hence MockMvc.
 *
 * `standaloneSetup` on purpose: this is about one advice against the servlet's
 * resolver chain, so a probe controller keeps the test independent of any real
 * route, security rule or database.
 */
class GlobalExceptionHandlerMvcTest {

    data class ProbeBody(
        @field:Positive(message = "n must be positive")
        val n: Int?,
    )

    @RestController
    @RequestMapping("/api/probe")
    class ProbeController {

        @GetMapping("/{id}")
        fun byId(@PathVariable id: Long): String = "ok"

        @PostMapping("/body")
        fun withBody(@Valid @RequestBody body: ProbeBody): String = "ok"

        @GetMapping("/denied")
        fun denied(): String = throw AccessDeniedException("Access Denied")

        @GetMapping("/anonymous")
        fun anonymous(): String =
            throw AuthenticationCredentialsNotFoundException("An Authentication object was not found")

        @GetMapping("/missing")
        fun missing(): String = throw NotFoundException("No tournament with id 99")

        @GetMapping("/boom")
        fun boom(): String = throw IllegalStateException("something nobody anticipated")
    }

    private val mvc = MockMvcBuilders.standaloneSetup(ProbeController())
        .setControllerAdvice(GlobalExceptionHandler())
        .setValidator(LocalValidatorFactoryBean().apply { afterPropertiesSet() })
        .build()

    @Test
    fun `a path variable that will not parse is a 400, not a 500`() {
        val response = mvc.perform(get("/api/probe/not-a-number")).andReturn().response

        assertEquals(400, response.status)
    }

    @Test
    fun `a malformed request body is a 400, not a 500`() {
        val response = mvc.perform(
            post("/api/probe/body").contentType(MediaType.APPLICATION_JSON).content("{ not json ")
        ).andReturn().response

        assertEquals(400, response.status)
    }

    @Test
    fun `an unsupported method is a 405, not a 500`() {
        val response = mvc.perform(delete("/api/probe/1")).andReturn().response

        assertEquals(405, response.status)
    }

    /**
     * The `@PreAuthorize` layer's denial, which method security raises from
     * inside the dispatch where `ExceptionTranslationFilter` cannot see it.
     */
    @Test
    fun `a method-security denial is a 403, not a 500`() {
        val response = mvc.perform(get("/api/probe/denied")).andReturn().response

        assertEquals(403, response.status)
        assertContains(response.bodyText(), "https://umfl.dev/problems/forbidden")
    }

    @Test
    fun `a method-security call with no authentication at all is a 401, not a 500`() {
        val response = mvc.perform(get("/api/probe/anonymous")).andReturn().response

        assertEquals(401, response.status)
        assertContains(response.bodyText(), "https://umfl.dev/problems/unauthorized")
    }

    @Test
    fun `bean validation still reports the offending fields`() {
        val response = mvc.perform(
            post("/api/probe/body").contentType(MediaType.APPLICATION_JSON).content("""{"n": -1}""")
        ).andReturn().response

        assertEquals(400, response.status)
        assertContains(response.bodyText(), "https://umfl.dev/problems/validation-failed")
        assertContains(response.bodyText(), "n must be positive")
    }

    /** Inheriting the base class must not have shadowed the domain handlers. */
    @Test
    fun `a domain NotFoundException is still a 404`() {
        val response = mvc.perform(get("/api/probe/missing")).andReturn().response

        assertEquals(404, response.status)
        assertContains(response.bodyText(), "https://umfl.dev/problems/not-found")
    }

    /** The catch-all still catches what is genuinely unexpected. */
    @Test
    fun `an unanticipated exception is still a 500`() {
        val response = mvc.perform(get("/api/probe/boom")).andReturn().response

        assertEquals(500, response.status)
        assertContains(response.bodyText(), "https://umfl.dev/problems/internal-error")
    }

    /** Spring's own problem bodies get the same `type` vocabulary as the hand-written ones. */
    @Test
    fun `a framework-raised problem carries a umfl problem type`() {
        val response = mvc.perform(delete("/api/probe/1")).andReturn().response

        assertContains(response.bodyText(), "https://umfl.dev/problems/method-not-allowed")
    }

    private fun MockHttpServletResponse.bodyText(): String = contentAsString
}
