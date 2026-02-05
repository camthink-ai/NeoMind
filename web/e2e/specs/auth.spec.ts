/**
 * Authentication E2E tests
 */

import { test, expect } from '@playwright/test';
import { LoginPage } from '../pages/LoginPage';
import { login, logout, isLoggedIn } from '../fixtures/auth';

test.describe('Authentication', () => {
  let loginPage: LoginPage;

  test.beforeEach(async ({ page }) => {
    loginPage = new LoginPage(page);
    await loginPage.goto();
  });

  test('should display login form', async ({ page }) => {
    await expect(page.locator('#username')).toBeVisible();
    await expect(page.locator('#password')).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toBeVisible();
  });

  test('should show error with invalid credentials', async ({ page }) => {
    // Manually enter invalid credentials (don't use loginPage.login() which waits for navigation)
    await page.locator('#username').click();
    await page.locator('#username').clear();
    await page.locator('#username').type('invalid', { delay: 10 });
    await page.locator('#password').click();
    await page.locator('#password').clear();
    await page.locator('#password').type('invalid', { delay: 10 });
    await page.click('button[type="submit"]');
    await page.waitForTimeout(1000);

    // Should stay on login page or show error
    const url = page.url();
    const hasError = await loginPage.getErrorMessage();

    expect(url.includes('/login') || hasError.length > 0).toBeTruthy();
  });

  test('should login successfully with valid credentials', async ({ page }) => {
    await login(page);

    // Should redirect to chat page (root path /)
    const url = page.url();
    expect(url.includes('/login')).toBeFalsy();

    // Should not show login form anymore
    const usernameVisible = await page.locator('#username').isVisible().catch(() => false);
    expect(usernameVisible).toBeFalsy();
  });

  test('should persist session across page reloads', async ({ page }) => {
    await login(page);

    // Reload page
    await page.reload();

    // Should still be logged in
    const stillLoggedIn = await isLoggedIn(page);
    expect(stillLoggedIn).toBeTruthy();
  });
});
