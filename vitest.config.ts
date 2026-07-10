import { defineConfig } from 'vitest/config'

// Vitest scoped to src/ unit tests. Playwright specs in tests/e2e/ are
// intentionally excluded — they run via `npx playwright test`.
export default defineConfig({
  test: {
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    exclude: ['tests/**', 'node_modules/**', 'dist/**', 'src-tauri/**'],
    environment: 'node',
  },
})
