package com.umfl.common

import org.junit.jupiter.api.Test
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.http.HttpStatus
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
        val problem = GlobalExceptionHandler().handleAsyncRequestNotUsable(
            AsyncRequestNotUsableException("Servlet container error notification for disconnected client")
        )

        assertNull(problem)
    }
}
