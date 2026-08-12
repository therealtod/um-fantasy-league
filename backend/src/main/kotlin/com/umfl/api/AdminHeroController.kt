package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.hero.AdminHeroService
import com.umfl.hero.HeroPoolEntryInput
import com.umfl.hero.HeroRepository
import com.umfl.manager.Manager
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

    /**
     * The admin-scoped view of a tournament's hero pool, distinct from the
     * public `/api/tournaments/{id}/heroes` the Roster Builder reads — the two
     * happen to return the same shape today but are separate endpoints so an
     * admin-only filter (e.g. "heroes NOT yet in this pool") can be added later
     * without reshaping the player-facing one. Compare `AdminMapController.listPool`.
     */
    @GetMapping("/api/admin/tournaments/{tournamentId}/heroes")
    fun listPool(
        @PathVariable tournamentId: Long,
        @CurrentManager admin: Manager,
    ): List<HeroDto> = adminHeroService.pool(tournamentId).map(HeroDto::from)

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

    /** Batch counterpart to [setPoolCost] — adds/re-prices several heroes in one request. */
    @PostMapping("/api/admin/tournaments/{tournamentId}/heroes")
    fun addBatchToPool(
        @PathVariable tournamentId: Long,
        @Valid @RequestBody request: AddHeroesToPoolRequest,
        @CurrentManager admin: Manager,
    ): List<HeroDto> = adminHeroService.addBatchToPool(
        tournamentId,
        request.heroes.orEmpty().map { HeroPoolEntryInput(requireNotNull(it.heroId), requireNotNull(it.cost)) },
    ).map(HeroDto::from)

    @PutMapping("/api/admin/tournaments/{tournamentId}/heroes/{heroId}")
    fun setPoolCost(
        @PathVariable tournamentId: Long,
        @PathVariable heroId: Long,
        @Valid @RequestBody request: SetHeroCostRequest,
        @CurrentManager admin: Manager,
    ): HeroDto = HeroDto.from(adminHeroService.setPoolCost(tournamentId, heroId, requireNotNull(request.cost)))

    @DeleteMapping("/api/admin/tournaments/{tournamentId}/heroes/{heroId}")
    @ResponseStatus(HttpStatus.NO_CONTENT)
    fun removeFromPool(
        @PathVariable tournamentId: Long,
        @PathVariable heroId: Long,
        @CurrentManager admin: Manager,
    ) = adminHeroService.removeFromPool(tournamentId, heroId)
}
