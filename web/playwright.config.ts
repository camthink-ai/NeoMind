import { defineConfig, devices } from '@playwright/test'

const headed = process.env.HEADED === '1' || process.env.PWHEADED === '1'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ['line'],
    ['json', { outputFile: 'test-results/mention-a11y-playwright.json' }],
  ],
  use: {
    // Prefer system Chrome when available; channel falls back for CI.
    channel: process.env.PWCHANNEL || 'chrome',
    headless: !headed,
    baseURL: process.env.PW_BASE_URL || 'http://127.0.0.1:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chrome',
      use: {
        ...devices['Desktop Chrome'],
        launchOptions: {
          args: ['--start-maximized'],
        },
      },
    },
  ],
  webServer: {
    command: 'npm run dev -- --host 127.0.0.1 --port 5173',
    url: 'http://127.0.0.1:5173',
    reuseExistingServer: true,
    timeout: 120_000,
  },
})
