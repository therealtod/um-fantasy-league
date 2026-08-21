import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vitest/config'
import { loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig(({ mode }) => {
  // Only VITE_-prefixed vars are loaded (loadEnv's default), which already
  // matches VITE_API_PROXY_TARGET below. Reads .env.local too, not just the
  // shell, so a tunnel target doesn't have to live in this file.
  const env = loadEnv(mode, process.cwd())

  return {
    plugins: [vue(), tailwindcss()],
    resolve: {
      alias: {
        '@': fileURLToPath(new URL('./src', import.meta.url)),
      },
    },
    server: {
      port: 5173,
      proxy: {
        // Keeps the browser on a single origin in development. Defaults to
        // a local backend; set VITE_API_PROXY_TARGET (in .env.local or the
        // shell) to point at a tunnel instead of editing this file.
        '/api': {
          target: env.VITE_API_PROXY_TARGET || 'http://localhost:8080',
          changeOrigin: true,
        },
      },
    },
    test: {
      environment: 'jsdom',
      include: ['src/**/*.spec.ts'],
    },
  }
})
