/// Upgrade path for the legacy "remember me" password.
///
/// Until this fix the login page stored `{ username, password }` in plaintext
/// under `neomind_remembered_credentials`. A long-term "remember me" user who
/// upgrades while still signed in lands straight in Chat and never sees the
/// login page — so the purge runs at app startup, before any community or
/// extension bundle can load. No live backend: API and auth are mocked.
import { expect, test, type Page } from '@playwright/test'
import { installMentionA11yMocks } from '../fixtures/mentionMockApi'

const LEGACY_KEY = 'neomind_remembered_credentials'
const OLD_PASSWORD = 'legacy-plaintext-password'

// Where an upgraded user can land without ever seeing the login page: the app
// root (which is the chat page) and a session URL.
const ENTRY_POINTS = ['/', '/chat/mention-a11y-session']

async function expectSignedInChatWithoutLegacyPassword(page: Page) {
  // Still signed in: no bounce to the login page, and the composer is up and usable.
  await expect(page).not.toHaveURL(/\/login/)
  const composer = page.locator('[contenteditable=true]').first()
  await expect(composer).toBeVisible({ timeout: 15000 })
  await expect(composer).toBeEditable()

  // The session credential survived …
  expect(await page.evaluate(() => sessionStorage.getItem('neomind_token_session'))).toBe('mention-a11y-token')

  // … and only the legacy password was removed.
  expect(await page.evaluate((key) => localStorage.getItem(key), LEGACY_KEY)).toBeNull()

  // Belt and braces: nothing left in storage carries the old password.
  const leaked = await page.evaluate((password) => {
    const values: string[] = []
    for (const storage of [localStorage, sessionStorage]) {
      for (let i = 0; i < storage.length; i++) {
        values.push(storage.getItem(storage.key(i) ?? '') ?? '')
      }
    }
    return values.filter((value) => value.includes(password))
  }, OLD_PASSWORD)
  expect(leaked).toEqual([])
}

for (const path of ENTRY_POINTS) {
  test(`upgrade: a still-signed-in user opening ${path} lands in Chat with the legacy password purged`, async ({ page }) => {
    await installMentionA11yMocks(page)
    await page.addInitScript(
      ({ key, password }) => {
        localStorage.setItem(key, JSON.stringify({ username: 'a11y-runner', password, rememberMe: true }))
      },
      { key: LEGACY_KEY, password: OLD_PASSWORD },
    )

    await page.goto(path)

    await expectSignedInChatWithoutLegacyPassword(page)
  })
}
