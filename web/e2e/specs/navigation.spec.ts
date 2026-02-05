/**
 * Navigation and Layout E2E tests
 */

import { test, expect } from '@playwright/test';
import { login } from '../fixtures/auth';

test.describe('Navigation', () => {

  test('should display top navigation', async ({ page }) => {
    await login(page);

    const nav = page.locator('nav, header');
    await expect(nav.first()).toBeVisible();
  });

  test('should have navigation links', async ({ page }) => {
    await login(page);

    // Check for common navigation items
    const chatLink = page.locator('a[href*="/chat"], button:has-text("聊天"), button:has-text("Chat")');
    const deviceLink = page.locator('a[href*="/device"], button:has-text("设备"), button:has-text("Device")');
    const agentLink = page.locator('a[href*="/agent"], button:has-text("智能体"), button:has-text("Agent")');

    expect(await chatLink.count() + await deviceLink.count() + await agentLink.count()).toBeGreaterThan(0);
  });

  test('should navigate between pages', async ({ page }) => {
    await login(page);

    // Try to navigate to different pages
    const pages = ['/chat', '/devices', '/agents', '/automation'];

    for (const path of pages) {
      await page.goto(path);
      await page.waitForLoadState('networkidle');

      // Verify we're on the correct page
      const url = page.url();
      expect(url).toContain(path);
    }
  });

  test('should have working back button', async ({ page }) => {
    await login(page);
    await page.goto('/devices');

    await page.goBack();
    await page.waitForLoadState('networkidle');

    // Should be back on previous page (likely / or /chat)
    const url = page.url();
    expect(url.includes('/devices')).toBeFalsy();
  });
});
