package com.umfl.map

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.tournament.TournamentService
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional

@Service
class AdminMapService(
    private val gameMapRepository: GameMapRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
    private val tournamentService: TournamentService,
) {

    @Transactional
    fun create(name: String): GameMap {
        if (gameMapRepository.findByName(name) != null) {
            throw ConflictException("A map named '$name' already exists.")
        }
        return gameMapRepository.save(GameMap(name = name))
    }

    @Transactional
    fun update(mapId: Long, name: String): GameMap {
        val existing = requireMap(mapId)
        val collision = gameMapRepository.findByName(name)
        if (collision != null && collision.id != mapId) {
            throw ConflictException("A map named '$name' already exists.")
        }
        return gameMapRepository.save(existing.copy(name = name))
    }

    /** Adds [mapId] to [tournamentId]'s board pool. Idempotent — there is nothing to "re-price". */
    @Transactional
    fun addToPool(tournamentId: Long, mapId: Long): GameMap {
        tournamentService.requireTournament(tournamentId)
        val map = requireMap(mapId)
        mapPoolAdminRepository.addToPool(tournamentId, mapId)
        return map
    }

    private fun requireMap(mapId: Long): GameMap =
        gameMapRepository.findById(mapId).orElseThrow { NotFoundException("No map with id $mapId") }
}
