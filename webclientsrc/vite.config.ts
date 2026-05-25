import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { resolve } from 'node:path'
import { runtimeConfigPlugin } from './scripts/vite-runtime-config.ts'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss(), runtimeConfigPlugin()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'silent-renew': resolve(__dirname, 'silent-renew.html'),
      },
    },
  },
  test: {
    globals: true,
    environment: 'happy-dom',
    setupFiles: ['./src/test/setup.ts'],
    css: true,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
      exclude: [
        'src/generated/**',
        'src/test/**',
        'src/**/*.test.tsx',
        'src/**/*.test.ts',
        'scripts/**',
        '*.config.*',
        '**/node_modules/**',
      ],
    },
  },
})
