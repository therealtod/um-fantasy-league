import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

// Flat config, matching the discipline `cargo fmt --check`/`cargo clippy` apply on the
// backend: this is the merge gate for frontend-ci.yml, not a style reformatter — a lint
// step here should catch mistakes, not opinions.
export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,tsx,vue}'],
  },
  {
    name: 'app/files-to-ignore',
    ignores: ['**/dist/**', '**/dist-ssr/**', '**/coverage/**', '**/playwright-report/**'],
  },
  // `essential` only, not `recommended` — the latter folds in the `strongly-recommended`
  // formatting tier (attribute line-wrapping, self-closing tags, ...), which would fight
  // this codebase's existing style: the linter here preserves formatting and catches
  // mechanical slips; `vue/attributes-order` below is added deliberately on top.
  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,
  {
    name: 'app/rules',
    rules: {
      // Catches a prop/computed/data member declared but never read in the template —
      // the class of bug a template-blind `noUnusedLocals` in tsconfig.json can't see.
      'vue/no-unused-properties': [
        'error',
        {
          groups: ['props', 'data', 'computed', 'methods', 'setup'],
        },
      ],
      // Enforces a single canonical order for v-if/v-for/binding/event attributes so
      // ordering bugs (e.g. an emit wired before the state it depends on) are consistent
      // and reviewable rather than incidental.
      'vue/attributes-order': 'error',
      // Allow a deliberately-unused parameter/variable to opt out with the conventional
      // `_` prefix (matches this codebase's existing usage, e.g. `_e` in test callbacks).
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    },
  },
)
