import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { Session } from '@supabase/supabase-js'
import { supabase } from '@/lib/supabaseClient'
import { isDevelopmentIdentityMode } from '@/lib/developmentIdentity'
import { isSafeRedirectPath } from '@/lib/redirectPath'
import { useManagerStore } from '@/stores/manager'
import { useRosterStore } from '@/stores/roster'

// Supabase reports a failed OAuth callback (e.g. the provider rejecting the
// code exchange) as `error`/`error_description` query and hash params on the
// `redirectTo` URL rather than throwing — supabase-js itself never surfaces
// these to the caller, so without this the failure is silently invisible.
function consumeAuthCallbackError(): string | null {
  const query = new URLSearchParams(window.location.search)
  const fragment = new URLSearchParams(window.location.hash.slice(1))
  const description = query.get('error_description') ?? fragment.get('error_description')
  const code = query.get('error') ?? fragment.get('error')
  if (!description && !code) return null

  for (const key of ['error', 'error_code', 'error_description']) {
    query.delete(key)
    fragment.delete(key)
  }
  const url = new URL(window.location.href)
  url.search = query.toString()
  url.hash = fragment.toString()
  window.history.replaceState(null, '', url.toString())

  return description ?? code
}

export const useAuthStore = defineStore('auth', () => {
  const session = ref<Session | null>(null)
  const initialized = ref(false)
  const error = ref<string | null>(null)
  const isAuthenticated = computed(() => isDevelopmentIdentityMode || session.value !== null)

  // A session dropping — an explicit sign-out or a token expiring underneath
  // the app, both delivered through `onAuthStateChange` above — must clear
  // every store that holds the previous manager's identity or private data.
  // A watcher here is the one place that can't be forgotten at a call site;
  // scattering `reset()` calls across `signOut()` callers would miss the
  // expiry path.
  watch(
    session,
    (next, previous) => {
      if (previous && !next) {
        useManagerStore().reset()
        useRosterStore().clearSession()
      }
    },
    // Synchronous: there is no DOM to batch against, and the header must stop
    // showing the previous manager's handle the instant the session drops.
    { flush: 'sync' },
  )

  async function init() {
    if (isDevelopmentIdentityMode) {
      initialized.value = true
      return
    }

    const callbackError = consumeAuthCallbackError()
    if (callbackError) error.value = callbackError

    const { data } = await supabase.auth.getSession()
    session.value = data.session
    supabase.auth.onAuthStateChange((_event, newSession) => {
      session.value = newSession
    })
    initialized.value = true
  }

  async function signInWithDiscord(redirectPath?: string) {
    if (isDevelopmentIdentityMode) return

    error.value = null
    const path = redirectPath && isSafeRedirectPath(redirectPath) ? redirectPath : ''
    const { error: signInError } = await supabase.auth.signInWithOAuth({
      provider: 'discord',
      options: { redirectTo: `${window.location.origin}${path}` },
    })
    if (signInError) error.value = signInError.message
  }

  async function signOut() {
    if (isDevelopmentIdentityMode) return
    await supabase.auth.signOut()
  }

  return {
    session,
    initialized,
    error,
    isAuthenticated,
    isDevelopmentIdentityMode,
    init,
    signInWithDiscord,
    signOut,
  }
})
