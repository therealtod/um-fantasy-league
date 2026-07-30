package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import com.umfl.tournament.AdminTournamentService
import com.umfl.tournament.TournamentService
import jakarta.validation.Valid
import org.springframework.http.HttpStatus
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.DeleteMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.ResponseStatus
import org.springframework.web.bind.annotation.RestController

/** Admin-only: create, update, and delete tournaments. */
@RestController
@RequestMapping("/api/admin/tournaments")
@PreAuthorize("hasRole('ADMIN')")
class AdminTournamentController(
    private val adminTournamentService: AdminTournamentService,
    private val tournamentService: TournamentService,
) {

    @PostMapping
    @ResponseStatus(HttpStatus.CREATED)
    fun create(
        @Valid @RequestBody request: CreateTournamentRequest,
        @CurrentManager admin: Manager,
    ): TournamentDto = TournamentDto.from(
        tournament = adminTournamentService.create(
            name = requireNotNull(request.name),
            format = requireNotNull(request.format),
            status = requireNotNull(request.status),
            startDate = requireNotNull(request.startDate),
            endDate = request.endDate,
            capacity = requireNotNull(request.capacity),
            rosterSize = requireNotNull(request.rosterSize),
            creditGrant = requireNotNull(request.creditGrant),
        ),
        enrolled = 0,
        myEntryStatus = null,
    )

    @PutMapping("/{id}")
    fun update(
        @PathVariable id: Long,
        @Valid @RequestBody request: UpdateTournamentRequest,
        @CurrentManager admin: Manager,
    ): TournamentDto = TournamentDto.from(
        tournament = adminTournamentService.update(
            tournamentId = id,
            name = requireNotNull(request.name),
            format = requireNotNull(request.format),
            status = requireNotNull(request.status),
            startDate = requireNotNull(request.startDate),
            endDate = request.endDate,
            capacity = requireNotNull(request.capacity),
            rosterSize = requireNotNull(request.rosterSize),
            creditGrant = requireNotNull(request.creditGrant),
        ),
        enrolled = tournamentService.enrolmentCount(id),
        myEntryStatus = null,
    )

    @DeleteMapping("/{id}")
    @ResponseStatus(HttpStatus.NO_CONTENT)
    fun delete(
        @PathVariable id: Long,
        @CurrentManager admin: Manager,
    ) {
        adminTournamentService.delete(id)
    }
}
