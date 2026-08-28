/**
 * Authentication fixtures for E2E tests
 */

import { Page } from '@playwright/test';

const TEST_USER = {
  username: 'admin',
  password: 'admin123',
};

/**
 * Ensure test user exists.
 * Self-registration is closed by default; the test account is the first-run
 * admin created via the setup wizard (only available while no users exist).
 * Uses relative URL to go through Vite proxy
 */
async function ensureTestUser(page: Page): Promise<boolean> {
  try {
    const statusResponse = await page.request.get('/api/setup/status');
    const setup = await statusResponse.json().catch(() => null);

    if (!setup?.setup_required) {
      // Setup already completed in a prior run — assume the account exists.
      return true;
    }

    const response = await page.request.post('/api/setup/initialize', {
      data: {
        username: TEST_USER.username,
        password: TEST_USER.password,
      },
    });

    // 200 = admin created just now; 403 SETUP_ALREADY_COMPLETED = lost the
    // race against a parallel worker — the account exists either way.
    const status = response.status();
    if (status === 200 || status === 201) {
      console.log('Test admin created via setup wizard');
      return true;
    } else if (status === 403) {
      console.log('Setup already completed by another worker');
      return true;
    } else {
      const text = await response.text();
      console.log('Setup initialize response:', status, text);
      return false;
    }
  } catch (e) {
    console.log('Setup initialize error:', (e as Error).message);
    return false;
  }
}

/**
 * Login with test credentials
 */
export async function login(page: Page): Promise<void> {
  // First ensure the test admin exists (idempotent via setup wizard)
  await ensureTestUser(page);

  await page.goto('/login');

  // Wait for login form to be visible
  await page.waitForSelector('#username', { timeout: 5000 });

  // Fill in credentials - use type to trigger React events
  const usernameInput = page.locator('#username');
  const passwordInput = page.locator('#password');

  await usernameInput.clear();
  await usernameInput.type(TEST_USER.username, { delay: 10 });

  await passwordInput.clear();
  await passwordInput.type(TEST_USER.password, { delay: 10 });

  // Submit form
  await page.click('button[type="submit"]');

  // Wait for navigation - login should redirect away from /login
  // The redirect goes to / which is the chat page
  await page.waitForFunction(() => {
    const url = window.location.href;
    return !url.includes('/login');
  }, { timeout: 10000 });

  // Wait a bit for the page to stabilize
  await page.waitForTimeout(500);
}

/**
 * Logout from the application
 */
export async function logout(page: Page): Promise<void> {
  // Click user menu or logout button
  const logoutButton = page.locator('button:has-text("退出"), button:has-text("Logout"), [data-testid="logout-button"]');
  if (await logoutButton.count() > 0) {
    await logoutButton.first().click();
    await page.waitForURL('/login', { timeout: 5000 });
  }
}

/**
 * Setup authenticated page - performs login and returns the page
 */
export async function setupAuthenticatedPage(page: Page): Promise<Page> {
  await login(page);
  return page;
}

/**
 * Check if user is logged in
 */
export async function isLoggedIn(page: Page): Promise<boolean> {
  const url = page.url();
  // Not on login page = logged in
  return !url.includes('/login');
}

/**
 * Get test user credentials
 */
export function getTestUser() {
  return { ...TEST_USER };
}
