import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: 'tests/e2e',
  use: { baseURL: 'http://localhost:5199' },
  webServer: { command: 'pnpm vite --port 5199 --strictPort', url: 'http://localhost:5199' },
})
