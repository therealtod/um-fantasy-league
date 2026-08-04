package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import com.umfl.map.AdminMapService
import com.umfl.map.GameMapRepository
import com.umfl.map.MapPoolAdminRepository
import jakarta.validation.Valid
import org.springframework.http.HttpStatus
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.DeleteMapping
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.ResponseStatus
import org.springframework.web.bind.annotation.RestController

/** Admin-only: create/update maps, and manage a tournament's board pool. */
@RestController
@PreAuthorize("hasRole('ADMIN')")
class AdminMapController(
    private val adminMapService: AdminMapService,
    private val gameMapRepository: GameMapRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
) {

    @GetMapping("/api/admin/maps")
    fun list(@CurrentManager admin: Manager): List<MapAdminDto> =
        gameMapRepository.findAll().map(MapAdminDto::from)

    @GetMapping("/api/admin/tournaments/{tournamentId}/maps")
    fun listPool(
        @PathVariable tournamentId: Long,
        @CurrentManager admin: Manager,
    ): List<MapAdminDto> = mapPoolAdminRepository.poolMaps(tournamentId).map(MapAdminDto::from)

    @PostMapping("/api/admin/maps")
    @ResponseStatus(HttpStatus.CREATED)
    fun create(
        @Valid @RequestBody request: CreateMapRequest,
        @CurrentManager admin: Manager,
    ): MapAdminDto = MapAdminDto.from(adminMapService.create(requireNotNull(request.name)))

    @PutMapping("/api/admin/maps/{id}")
    fun update(
        @PathVariable id: Long,
        @Valid @RequestBody request: UpdateMapRequest,
        @CurrentManager admin: Manager,
    ): MapAdminDto = MapAdminDto.from(adminMapService.update(id, requireNotNull(request.name)))

    @PutMapping("/api/admin/tournaments/{tournamentId}/maps/{mapId}")
    fun addToPool(
        @PathVariable tournamentId: Long,
        @PathVariable mapId: Long,
        @CurrentManager admin: Manager,
    ): MapAdminDto = MapAdminDto.from(adminMapService.addToPool(tournamentId, mapId))

    @DeleteMapping("/api/admin/tournaments/{tournamentId}/maps/{mapId}")
    @ResponseStatus(HttpStatus.NO_CONTENT)
    fun removeFromPool(
        @PathVariable tournamentId: Long,
        @PathVariable mapId: Long,
        @CurrentManager admin: Manager,
    ) = adminMapService.removeFromPool(tournamentId, mapId)
}
