package com.umfl.auth

import com.umfl.manager.Manager
import com.umfl.manager.ManagerRepository
import com.umfl.manager.RankDivision
import com.umfl.manager.RankTier
import org.junit.jupiter.api.Test
import org.mockito.ArgumentMatchers.any
import org.mockito.Mockito.mock
import org.mockito.Mockito.`when`
import org.springframework.dao.DuplicateKeyException
import org.springframework.security.oauth2.jwt.Jwt
import org.springframework.security.oauth2.jwt.JwtClaimNames
import java.time.Instant
import java.util.UUID
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertSame
import kotlin.test.assertTrue

class SupabaseAuthenticationConverterTest {

    private val managerRepository = mock(ManagerRepository::class.java)
    private val converter = SupabaseAuthenticationConverter(managerRepository)

    private fun jwt(subject: UUID, metadata: Map<String, Any> = emptyMap()) =
        Jwt.withTokenValue("token")
            .header("alg", "RS256")
            .claim(JwtClaimNames.SUB, subject.toString())
            .claim("user_metadata", metadata)
            .issuedAt(Instant.now())
            .expiresAt(Instant.now().plusSeconds(3600))
            .build()

    @Test
    fun `returns the existing manager linked to the auth user id`() {
        val authUserId = UUID.randomUUID()
        val existing = Manager(
            id = 1,
            handle = "Existing",
            displayName = "Existing",
            rankTier = RankTier.GOLD,
            rankDivision = RankDivision.I,
            authUserId = authUserId,
        )
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(existing)

        val token = converter.convert(jwt(authUserId))

        assertSame(existing, token.principal)
    }

    @Test
    fun `provisions a new manager on first login using the discord username`() {
        val authUserId = UUID.randomUUID()
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(null)
        `when`(managerRepository.findByHandle("CoolDiscordUser")).thenReturn(null)
        `when`(managerRepository.save(any(Manager::class.java))).thenAnswer { it.arguments[0] as Manager }

        val result = converter.convert(jwt(authUserId, mapOf("user_name" to "CoolDiscordUser"))).principal as Manager

        assertEquals("CoolDiscordUser", result.handle)
        assertEquals("CoolDiscordUser", result.displayName)
        assertEquals(RankTier.BRONZE, result.rankTier)
        assertEquals(RankDivision.III, result.rankDivision)
        assertEquals(authUserId, result.authUserId)
        assertFalse(result.isAdmin, "a JIT-provisioned manager is never an admin by default")
    }

    @Test
    fun `appends a numeric suffix when the derived handle is already taken`() {
        val authUserId = UUID.randomUUID()
        val collision = Manager(
            id = 2,
            handle = "Taken",
            displayName = "Taken",
            rankTier = RankTier.SILVER,
            rankDivision = RankDivision.II,
        )
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(null)
        `when`(managerRepository.findByHandle("Taken")).thenReturn(collision)
        `when`(managerRepository.findByHandle("Taken1")).thenReturn(null)
        `when`(managerRepository.save(any(Manager::class.java))).thenAnswer { it.arguments[0] as Manager }

        val result = converter.convert(jwt(authUserId, mapOf("user_name" to "Taken"))).principal as Manager

        assertEquals("Taken1", result.handle)
    }

    @Test
    fun `recovers by re-querying when a concurrent request already provisioned the same auth user`() {
        val authUserId = UUID.randomUUID()
        val winner = Manager(
            id = 4,
            handle = "CoolDiscordUser",
            displayName = "CoolDiscordUser",
            rankTier = RankTier.BRONZE,
            rankDivision = RankDivision.III,
            authUserId = authUserId,
        )
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(null, winner)
        `when`(managerRepository.findByHandle("CoolDiscordUser")).thenReturn(null)
        `when`(managerRepository.save(any(Manager::class.java))).thenThrow(DuplicateKeyException("manager_handle_key"))

        val result = converter.convert(jwt(authUserId, mapOf("user_name" to "CoolDiscordUser"))).principal as Manager

        assertSame(winner, result)
    }

    @Test
    fun `retries with a fresh handle when the collision is a different, still-unlinked auth user`() {
        val authUserId = UUID.randomUUID()
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(null)
        `when`(managerRepository.findByHandle("Taken")).thenReturn(null, Manager(
            id = 5,
            handle = "Taken",
            displayName = "Taken",
            rankTier = RankTier.SILVER,
            rankDivision = RankDivision.II,
        ))
        `when`(managerRepository.findByHandle("Taken1")).thenReturn(null)
        `when`(managerRepository.save(any(Manager::class.java)))
            .thenThrow(DuplicateKeyException("manager_handle_key"))
            .thenAnswer { it.arguments[0] as Manager }

        val result = converter.convert(jwt(authUserId, mapOf("user_name" to "Taken"))).principal as Manager

        assertEquals("Taken1", result.handle)
    }

    @Test
    fun `an admin manager's token carries ROLE_ADMIN`() {
        val authUserId = UUID.randomUUID()
        val admin = Manager(
            id = 3,
            handle = "AdminUser",
            displayName = "Admin User",
            rankTier = RankTier.ELITE,
            rankDivision = RankDivision.I,
            authUserId = authUserId,
            isAdmin = true,
        )
        `when`(managerRepository.findByAuthUserId(authUserId)).thenReturn(admin)

        val token = converter.convert(jwt(authUserId))

        assertTrue(token.authorities.any { it.authority == "ROLE_ADMIN" })
    }
}
