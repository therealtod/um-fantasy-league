package com.umfl.common

import com.umfl.match.MatchViolation
import com.umfl.scoring.ScoringViolation
import com.umfl.tournament.RosterViolation

sealed class DomainException(message: String) : RuntimeException(message)

/** The requested resource does not exist. Surfaces as 404. */
class NotFoundException(message: String) : DomainException(message)

/** The request is well-formed but conflicts with current state. Surfaces as 409. */
class ConflictException(message: String) : DomainException(message)

/** The server is at a deliberate capacity limit; the client should retry. Surfaces as 503. */
class ServiceUnavailableException(message: String) : DomainException(message)

/**
 * A roster action broke one or more league rules. Surfaces as 422 with the full
 * violation list, so the client can highlight every problem at once rather than
 * making the user discover them one at a time.
 */
class RosterRuleException(val violations: List<RosterViolation>) :
    DomainException(violations.joinToString("; ") { it.message })

/**
 * An admin match submission broke one or more result rules. Same shape as
 * [RosterRuleException], kept as its own type rather than a shared one so
 * roster and match violation reporting can evolve independently.
 */
class MatchRuleException(val violations: List<MatchViolation>) :
    DomainException(violations.joinToString("; ") { it.message })

/**
 * An admin scoring rule set submission broke one or more coefficient rules.
 * Same shape and reasoning as [MatchRuleException] — its own type so scoring's
 * violation vocabulary stays independent of roster's and match's.
 */
class ScoringRuleException(val violations: List<ScoringViolation>) :
    DomainException(violations.joinToString("; ") { it.message })
