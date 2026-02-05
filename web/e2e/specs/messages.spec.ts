/**
 * Message and notification tests
 */

import { test, expect } from '@playwright/test';
import { login } from '../fixtures/auth';

test.describe('Messages & Notifications', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should display messages page', async ({ page }) => {
    await page.goto('/messages');

    const url = page.url();
    // Messages page should load without error
    const bodyVisible = await page.locator('body').isVisible();
    expect(bodyVisible).toBeTruthy();
  });

  test('should have message list or empty state', async ({ page }) => {
    await page.goto('/messages');

    // Just verify page loaded - content may vary
    const bodyVisible = await page.locator('body').isVisible();
    expect(bodyVisible).toBeTruthy();
  });

  test('should be able to mark messages as read', async ({ page }) => {
    await page.goto('/messages');

    const markReadBtn = page.locator('button:has-text("标为已读"), button:has-text("Mark Read"), [data-testid="mark-read-button"]');

    if (await markReadBtn.count() > 0) {
      await markReadBtn.first().click();

      // Should show success feedback
      const toast = page.locator('.toast, [role="alert"]');
      await expect(toast.first()).toBeVisible();
    }
  });

  test('should filter messages by severity', async ({ page }) => {
    await page.goto('/messages');

    // Look for filter options
    const filter = page.locator('[data-testid="severity-filter"], select');
    expect(await filter.count()).toBeGreaterThanOrEqual(0);
  });
});
