package com.umfl.manager

import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository
import java.util.UUID

@Repository
interface ManagerRepository : CrudRepository<Manager, Long> {
    fun findByHandle(handle: String): Manager?
    fun findByAuthUserId(authUserId: UUID): Manager?
}
