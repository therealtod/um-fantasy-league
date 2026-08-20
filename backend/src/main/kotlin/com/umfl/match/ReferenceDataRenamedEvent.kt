package com.umfl.match

/**
 * A hero or a board was renamed.
 *
 * Renaming reference data is the one thing that can invalidate an assembled
 * [MatchResult] without going through [AdminMatchService]: `heroName` and
 * `mapName` are copies joined in when the match was assembled, not live reads,
 * so a rename leaves every cached copy spelling the old name. Nothing else on
 * either row reaches a match — `heroes.image_url` and `tournament_hero.cost` do
 * not — which is why this is published only when the name actually changed.
 *
 * An event rather than a direct call into [MatchResultCache] so a rename gets
 * the same before-and-after-completion invalidation pair a match write gets. A
 * bare call inside the writing method would only be the before half, leaving
 * the window between it and the commit open for a reader to cache the old name
 * back again — the same race the cache's version stamp exists to close.
 *
 * [renamed] is a description for a log line and nothing more: the cache drops
 * everything rather than hunting the id, since a hero or board belongs to no
 * single tournament.
 */
data class ReferenceDataRenamedEvent(val renamed: String)
