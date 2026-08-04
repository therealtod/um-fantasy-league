package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import com.umfl.scoring.AdminScoringService
import com.umfl.scoring.ScoringCoefficientInput
import com.umfl.scoring.ScoringRuleSetResult
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

/** Admin-only: create, update, list and activate a tournament's scoring rule sets. */
@RestController
@RequestMapping("/api/admin/tournaments/{tournamentId}/scoring-rule-sets")
@PreAuthorize("hasRole('ADMIN')")
class AdminScoringController(
    private val adminScoringService: AdminScoringService,
) {

    @GetMapping
    fun list(
        @PathVariable tournamentId: Long,
        @CurrentManager admin: Manager,
    ): List<ScoringRuleSetDto> = adminScoringService.list(tournamentId).map { it.toDto() }

    @PostMapping
    @ResponseStatus(HttpStatus.CREATED)
    fun create(
        @PathVariable tournamentId: Long,
        @Valid @RequestBody request: CreateScoringRuleSetRequest,
        @CurrentManager admin: Manager,
    ): ScoringRuleSetDto = adminScoringService.create(
        tournamentId = tournamentId,
        name = requireNotNull(request.name),
        coefficients = request.coefficients.orEmpty().map { it.toInput() },
        activate = request.activate,
    ).toDto()

    @PutMapping("/{ruleSetId}")
    fun update(
        @PathVariable tournamentId: Long,
        @PathVariable ruleSetId: Long,
        @Valid @RequestBody request: UpdateScoringRuleSetRequest,
        @CurrentManager admin: Manager,
    ): ScoringRuleSetDto = adminScoringService.update(
        tournamentId = tournamentId,
        ruleSetId = ruleSetId,
        name = requireNotNull(request.name),
        coefficients = request.coefficients.orEmpty().map { it.toInput() },
    ).toDto()

    @PostMapping("/{ruleSetId}/activate")
    fun activate(
        @PathVariable tournamentId: Long,
        @PathVariable ruleSetId: Long,
        @CurrentManager admin: Manager,
    ): ScoringRuleSetDto = ScoringRuleSetDto.from(adminScoringService.activate(tournamentId, ruleSetId))
}

private fun ScoringCoefficientRequest.toInput() = ScoringCoefficientInput(
    metric = requireNotNull(metric),
    coefficient = requireNotNull(coefficient),
    sortOrder = sortOrder,
)

private fun ScoringRuleSetResult.toDto() = ScoringRuleSetDto.from(ruleSet, unknownMetrics)
