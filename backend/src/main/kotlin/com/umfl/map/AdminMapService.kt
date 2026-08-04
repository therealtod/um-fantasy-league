package com.umfl.map

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.tournament.TournamentService
import org.springframework.dao.DataIntegrityViolationException
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

    /**
     * Removes [mapId] from [tournamentId]'s board pool. Rejected outright, as a
     * 409 rather than a raw FK error, if the tournament already has a recorded
     * match on that map — see [MapPoolAdminRepository.hasRecordedMatch]'s doc.
     *
     * The check and the delete are two statements, so a match recorded in
     * between slips past the check and is stopped by the FK instead. That
     * window is narrow but real, so the violation is translated into the same
     * [ConflictException] rather than left to `GlobalExceptionHandler`'s
     * catch-all, which would render it as the generic "should never fire" 409.
     */
    @Transactional
    fun removeFromPool(tournamentId: Long, mapId: Long) {
        tournamentService.requireTournament(tournamentId)
        requireMap(mapId)
        if (mapPoolAdminRepository.hasRecordedMatch(tournamentId, mapId)) {
            throw recordedMatchConflict(tournamentId, mapId)
        }

        val removed = try {
            mapPoolAdminRepository.removeFromPool(tournamentId, mapId)
        } catch (e: DataIntegrityViolationException) {
            throw recordedMatchConflict(tournamentId, mapId)
        }
        if (!removed) {
            throw NotFoundException("Map $mapId is not in tournament $tournamentId's pool")
        }
    }

    private fun recordedMatchConflict(tournamentId: Long, mapId: Long) = ConflictException(
        "Map $mapId has recorded matches in tournament $tournamentId and cannot be removed from its pool."
    )

    private fun requireMap(mapId: Long): GameMap =
        gameMapRepository.findById(mapId).orElseThrow { NotFoundException("No map with id $mapId") }
}
