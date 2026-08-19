package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import com.umfl.matchimport.MatchImportService
import jakarta.validation.Valid
import org.springframework.security.access.prepost.PreAuthorize
import org.springframework.web.bind.annotation.PathVariable
import org.springframework.web.bind.annotation.PostMapping
import org.springframework.web.bind.annotation.RequestBody
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RestController

/**
 * Admin-only: turn a Tabletop League match URL into a reviewable draft.
 *
 * Sits beside [AdminMatchController] rather than inside it because it does a
 * categorically different thing — this endpoint **writes nothing**. It scrapes,
 * resolves the source's hero and board names onto this league's rows, and
 * returns a draft. Recording it is still a separate, deliberate `POST` to
 * [AdminMatchController], which means every imported match goes through
 * `MatchResultPolicy` exactly as a hand-typed one does.
 *
 * Guarded twice, like every admin controller: the admin URL matcher in
 * [com.umfl.config.SecurityConfig] and the `@PreAuthorize` below.
 */
@RestController
@RequestMapping("/api/admin/tournaments/{tournamentId}/matches")
@PreAuthorize("hasRole('ADMIN')")
class AdminMatchImportController(private val matchImportService: MatchImportService) {

    /**
     * Scrapes [ImportMatchRequest.sourceUrl] and resolves it against
     * [tournamentId]'s catalogue and board pool.
     *
     * Answers 200 with unresolved names listed rather than failing on them: a
     * board missing from the pool or a hero missing from the catalogue is
     * something the admin fixes and re-imports, and the rest of the scrape is
     * still worth showing them. The genuinely broken cases — a URL that isn't a
     * match page, a scraper that can't read the page, a scraper that isn't
     * running — surface as 409 or 503 through [com.umfl.common.GlobalExceptionHandler].
     */
    @PostMapping("/import")
    fun import(
        @PathVariable tournamentId: Long,
        @Valid @RequestBody request: ImportMatchRequest,
        @CurrentManager admin: Manager,
    ): MatchImportPreviewDto =
        MatchImportPreviewDto.from(
            matchImportService.preview(tournamentId, requireNotNull(request.sourceUrl)),
        )
}
