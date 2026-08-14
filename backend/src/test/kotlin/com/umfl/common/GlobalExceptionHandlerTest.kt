package com.umfl.common

import org.junit.jupiter.api.Test
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.http.HttpStatus
import org.springframework.mock.web.MockHttpServletRequest
import org.springframework.web.context.request.ServletWebRequest
import org.springframework.web.context.request.async.AsyncRequestNotUsableException
import kotlin.test.assertEquals
import kotlin.test.assertNull

class GlobalExceptionHandlerTest {

    @Test
    fun `a data integrity violation surfaces as 409, not 500`() {
        val problem = GlobalExceptionHandler().handleDataIntegrityViolation(
            DataIntegrityViolationException("insert or update on table violates foreign key constraint")
        )

        assertEquals(HttpStatus.CONFLICT.value(), problem.status)
    }

    @Test
    fun `an async request going unusable on a disconnected SSE client renders no body`() {
        val response = TestGlobalExceptionHandler().asyncRequestNotUsable(
            AsyncRequestNotUsableException("Servlet container error notification for disconnected client"),
            ServletWebRequest(MockHttpServletRequest()),
        )

        assertNull(response)
    }

    /**
     * `handleAsyncRequestNotUsableException` overrides a `protected` hook on
     * `ResponseEntityExceptionHandler`, so only a subclass can call it — the
     * same reason it had to become an override in the first place (see
     * [GlobalExceptionHandler]'s class doc).
     */
    private class TestGlobalExceptionHandler : GlobalExceptionHandler() {
        fun asyncRequestNotUsable(ex: AsyncRequestNotUsableException, request: ServletWebRequest) =
            handleAsyncRequestNotUsableException(ex, request)
    }
}
