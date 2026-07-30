package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.common.NotFoundException
import com.umfl.manager.Manager
import com.umfl.tournament.AdminTournamentService
import com.umfl.tournament.EntryStatus
import com.umfl.tournament.TournamentEntryRepository
import com.umfl.tournament.TournamentService
import com.umfl.tournament.TournamentStatus
import jakarta.validation.Valid
import org.springframework.http.HttpStatus
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.DeleteMapping
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.PutMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RequestParam
import org.springframework.web.bind.annotation.ResponseStatus
import org.springframework.web.bind.annotation.RestController

@RestController
@RequestMapping("/api/tournaments")
class TournamentController(
    private val tournamentService: TournamentService,
    private val entryRepository: TournamentEntryRepository,
    private val adminTournamentService: AdminTournamentService,
) {

    @GetMapping
    fun list(
        @RequestParam(required = false) status: TournamentStatus?,
        @CurrentManager manager: Manager?,
    ): List<TournamentDto> {
        val enrolment = tournamentService.enrolmentCounts()
        val myEntries = myEntryStatuses(manager)
        return tournamentService.listTournaments(status).map { tournament ->
            TournamentDto.from(
                tournament = tournament,
                enrolled = enrolment[tournament.id] ?: 0,
                myEntryStatus = myEntries[tournament.id],
            )
        }
    }

    @GetMapping("/{id}")
    fun get(@PathVariable id: Long, @CurrentManager manager: Manager?): TournamentDto {
        val tournament = tournamentService.requireTournament(id)
        return TournamentDto.from(
            tournament = tournament,
            enrolled = tournamentService.enrolmentCount(id),
            myEntryStatus = myEntryStatuses(manager)[id],
        )
    }

    @DeleteMapping("/{id}")
    @ResponseStatus(HttpStatus.NO_CONTENT)
    @PreAuthorize("hasRole('ADMIN')")
    fun delete(@PathVariable id: Long, @CurrentManager admin: Manager) {
        adminTournamentService.delete(id)
    }

    /** Enter this tournament: opens an empty draft roster with the tournament's credit grant. */
    @PostMapping("/{id}/entries")
    @ResponseStatus(HttpStatus.CREATED)
    fun register(@PathVariable id: Long, @CurrentManager manager: Manager): RosterDto =
        RosterDto.from(tournamentService.register(id, manager))

    @GetMapping("/{id}/entries/me")
    fun myEntry(@PathVariable id: Long, @CurrentManager manager: Manager): RosterDto =
        tournamentService.findMyEntry(id, manager)?.let(RosterDto::from)
            ?: throw NotFoundException("You are not registered for tournament $id.")

    /**
     * Replace the roster selection. Over-budget drafts are accepted — the budget
     * is enforced when locking — but duplicates, oversized rosters and heroes
     * outside this tournament's pool are rejected with 422.
     */
    @PutMapping("/{id}/entries/me/slots")
    fun setSlots(
        @PathVariable id: Long,
        @Valid @RequestBody request: SetSlotsRequest,
        @CurrentManager manager: Manager,
    ): RosterDto = RosterDto.from(
        tournamentService.setSlots(id, manager, request.heroIds.orEmpty())
    )

    /** Commit the roster. Requires a full roster within the entry's credit grant. */
    @PostMapping("/{id}/entries/me/lock")
    fun lock(@PathVariable id: Long, @CurrentManager manager: Manager): RosterDto =
        RosterDto.from(tournamentService.lockRoster(id, manager))

    private fun myEntryStatuses(manager: Manager?): Map<Long, EntryStatus> =
        manager?.id
            ?.let { entryRepository.findByManagerId(it) }
            ?.associate { it.tournamentId to it.status }
            .orEmpty()
}
