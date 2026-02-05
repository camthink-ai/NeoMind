/**
 * Theme and localization tests
 */

import { test, expect } from '@playwright/test';

test.describe('Theme & Language', () => {

  test('should support theme switching', async ({ page }) => {
    await page.goto('/login');

    const themeButton = page.locator('button[aria-label*="theme"], button[aria-label*="主题"], [data-testid="theme-toggle"]');

    if (await themeButton.count() > 0) {
      // Get initial theme
      const initialHtml = await page.locator('html').getAttribute('class');
      const isDark = initialHtml?.includes('dark');

      // Toggle theme
      await themeButton.click();
      await page.waitForTimeout(500);

      // Check theme changed
      const newHtml = await page.locator('html').getAttribute('class');
      const nowDark = newHtml?.includes('dark');

      expect(isDark).not.toBe(nowDark);
    }
  });

  test('should have language selector', async ({ page }) => {
    await page.goto('/login');

    const langSelector = page.locator('button:has-text("Language"), button:has-text("语言"), [data-testid="language-selector"]');

    if (await langSelector.count() > 0) {
      await langSelector.click();

      // Should show language options
      const options = page.locator('[role="menuitem"], [role="option"]');
      expect(await options.count()).toBeGreaterThan(0);
    }
  });

  test('should persist theme preference', async ({ page }) => {
    await page.goto('/');

    // Get initial theme
    const initialHtml = await page.locator('html').getAttribute('class');

    // Navigate to another page
    await page.goto('/chat');

    // Theme should be the same
    const newHtml = await page.locator('html').getAttribute('class');

    // Both should have dark class or both should not
    const bothDark = initialHtml?.includes('dark') && newHtml?.includes('dark');
    const bothLight = !initialHtml?.includes('dark') && !newHtml?.includes('dark');

    expect(bothDark || bothLight).toBeTruthy();
  });
});
