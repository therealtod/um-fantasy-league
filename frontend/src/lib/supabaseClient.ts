import { createClient } from '@supabase/supabase-js'
import { isDevelopmentIdentityMode } from './developmentIdentity'

const supabaseUrl = import.meta.env.VITE_SUPABASE_URL
const supabaseAnonKey = import.meta.env.VITE_SUPABASE_ANON_KEY

if ((!supabaseUrl || !supabaseAnonKey) && !isDevelopmentIdentityMode) {
  throw new Error(
    'VITE_SUPABASE_URL and VITE_SUPABASE_ANON_KEY must be set (see frontend/.env.local.example)',
  )
}

// Development identity mode never calls Supabase, but retaining a client keeps
// the module API stable for shared frontend code and test mocks.
export const supabase = createClient(
  supabaseUrl ?? 'http://localhost:54321',
  supabaseAnonKey ?? 'development-identity-placeholder',
)
