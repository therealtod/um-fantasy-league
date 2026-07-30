package com.umfl.map

import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository

@Repository
interface GameMapRepository : CrudRepository<GameMap, Long> {
    fun findByName(name: String): GameMap?
}
