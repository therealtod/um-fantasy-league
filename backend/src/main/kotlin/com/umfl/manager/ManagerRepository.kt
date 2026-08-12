package com.umfl.manager

import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository
import java.util.UUID

@Repository
interface ManagerRepository : CrudRepository<Manager, Long> {
    fun findByHandle(handle: String): Manager?
    fun findByAuthUserId(authUserId: UUID): Manager?

    /**
     * Any one admin-flagged manager, lowest id first. Used only by
     * [com.umfl.auth.DevManagerAuthenticationFilter] to impersonate someone
     * with admin rights when a dev/test request carries no `X-Manager-Id` —
     * deliberately not keyed to a specific handle, so that fallback works
     * against whichever admin manager happens to exist rather than a
     * hardcoded name.
     */
    fun findFirstByIsAdminTrueOrderById(): Manager?
}
