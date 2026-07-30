package com.umfl.auth

import com.umfl.manager.Manager

/**
 * Resolves the manager making the current request.
 *
 * This is the seam where real authentication will land. Controllers depend only
 * on this interface, so swapping [DevManagerProvider] for a JWT- or OAuth-backed
 * implementation requires no changes above it.
 */
fun interface CurrentManagerProvider {
    fun current(): Manager

    /** Like [current], but `null` for anonymous requests instead of throwing. */
    fun currentOrNull(): Manager? = current()
}
