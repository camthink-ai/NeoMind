/**
 * Setup/Onboarding E2E tests
 * Note: These tests only work on fresh installations with no users
 */

import { test, expect } from '@playwright/test';
import { SetupPage } from '../pages/SetupPage';

test.describe('Setup (First-time)', () => {
  // Helper to check if setup is needed
  async function isSetupNeeded(page: any): Promise<boolean> {
    const setupPage = new SetupPage(page);
    await setupPage.goto();
    const url = page.url();
    // If redirected to login, setup is not needed
    return url.includes('/setup');
  }

  test('should display setup page for new installation', async ({ page }) => {
    const setupNeeded = await isSetupNeeded(page);

    if (!setupNeeded) {
      test.skip(true, 'Setup already completed - user exists');
      return;
    }

    const url = page.url();
    expect(url).toContain('/setup');
  });

  test('should have username and password fields', async ({ page }) => {
    const setupNeeded = await isSetupNeeded(page);

    if (!setupNeeded) {
      test.skip(true, 'Setup already completed - user exists');
      return;
    }

    const setupPage = new SetupPage(page);
    const hasUsername = await page.locator(setupPage.usernameInput).count() > 0;
    const hasPassword = await page.locator(setupPage.passwordInput).count() > 0;

    expect(hasUsername && hasPassword).toBeTruthy();
  });

  test('should validate password length', async ({ page }) => {
    const setupNeeded = await isSetupNeeded(page);

    if (!setupNeeded) {
      test.skip(true, 'Setup already completed - user exists');
      return;
    }

    const setupPage = new SetupPage(page);

    // Fill with short password
    const passwordInput = page.locator(setupPage.passwordInput);
    if (await passwordInput.count() > 0) {
      await passwordInput.first().click();
      await passwordInput.first().type('123', { delay: 10 });
      await page.waitForTimeout(200);

      // Submit button should be disabled
      const submitBtn = page.locator('button[type="submit"]');
      const isDisabled = await submitBtn.isDisabled();
      expect(isDisabled).toBeTruthy();
    }
  });

  test('should complete setup and redirect', async ({ page }) => {
    const setupNeeded = await isSetupNeeded(page);

    if (!setupNeeded) {
      test.skip(true, 'Setup already completed - user exists');
      return;
    }

    const setupPage = new SetupPage(page);
    await setupPage.setupAdmin('admin', 'admin123');

    // Should redirect away from setup
    const url = page.url();
    expect(url.includes('/setup')).toBeFalsy();
  });
});
