package com.umfl.manager

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.Table

/**
 * A human player of the fantasy league — the person who drafts a roster, as
 * distinct from the competitor who plays the real tournament (a free-text
 * `match_participant.player_label`, not an entity) and the `hero` they bring
 * to it. This is the only human the application actually models.
 *
 * There is no credit balance here. Budget is granted per registration
 * (`tournament_entry.credit_grant`), not held in a global wallet, so entering a
 * tournament costs nothing and cannot be blocked by a wallet running dry.
 */
@Table("manager")
data class Manager(
    @Id val id: Long? = null,
    val handle: String,
    val displayName: String,
    /** Supabase auth.users.id (JWT "sub" claim). Null for dev-seeded managers with no linked identity. */
    val authUserId: java.util.UUID? = null,
    /** Grants ROLE_ADMIN for admin-only routes. Our own data, independent of any auth provider. */
    val isAdmin: Boolean = false,
)
