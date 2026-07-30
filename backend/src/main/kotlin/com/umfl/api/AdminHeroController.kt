package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.hero.AdminHeroService
import com.umfl.hero.HeroRepository
import com.umfl.manager.Manager
import jakarta.validation.Valid
import org.springframework.http.HttpStatus
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.ResponseStatus
import org.springframework.web.bind.annotation.RestController

/** Admin-only: create/update hero identities, and manage a tournament's hero pool and pricing. */
@RestController
@PreAuthorize("hasRole('ADMIN')")
class AdminHeroController(
    private val adminHeroService: AdminHeroService,
    private val heroRepository: HeroRepository,
) {

    @GetMapping("/api/admin/heroes")
    fun list(@CurrentManager admin: Manager): List<HeroAdminDto> =
        heroRepository.findAll().map(HeroAdminDto::from)

    @PostMapping("/api/admin/heroes")
    @ResponseStatus(HttpStatus.CREATED)
    fun create(
        @Valid @RequestBody request: CreateHeroRequest,
        @CurrentManager admin: Manager,
    ): HeroAdminDto = HeroAdminDto.from(adminHeroService.create(requireNotNull(request.name), request.imageUrl))

    @PutMapping("/api/admin/heroes/{id}")
    fun update(
        @PathVariable id: Long,
        @Valid @RequestBody request: UpdateHeroRequest,
        @CurrentManager admin: Manager,
    ): HeroAdminDto = HeroAdminDto.from(adminHeroService.update(id, requireNotNull(request.name), request.imageUrl))

    @PutMapping("/api/admin/tournaments/{tournamentId}/heroes/{heroId}")
    fun setPoolCost(
        @PathVariable tournamentId: Long,
        @PathVariable heroId: Long,
        @Valid @RequestBody request: SetHeroCostRequest,
        @CurrentManager admin: Manager,
    ): HeroDto = HeroDto.from(adminHeroService.setPoolCost(tournamentId, heroId, requireNotNull(request.cost)))
}
