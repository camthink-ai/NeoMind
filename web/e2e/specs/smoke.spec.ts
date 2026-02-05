/**
 * Smoke tests - quick health checks
 */

import { test, expect } from '@playwright/test';

test.describe('Smoke Tests', () => {

  test('application loads without JavaScript errors', async ({ page }) => {
    const errors: string[] = [];

    page.on('pageerror', (error) => {
      errors.push(error.toString());
    });

    await page.goto('/');

    // Check for critical errors
    const criticalErrors = errors.filter(e =>
      e.includes('TypeError') ||
      e.includes('ReferenceError') ||
      e.includes('ChunkLoadError')
    );

    expect(criticalErrors.length).toBe(0);
  });

  test('all main routes are accessible', async ({ page }) => {
    const routes = ['/', '/login', '/chat', '/devices', '/agents', '/automation', '/settings'];

    for (const route of routes) {
      const response = await page.goto(route);
      // Should not error (could be 200, 302 redirect, or 401 for protected routes)
      expect(response?.status()).toBeLessThan(500);
    }
  });

  test('static assets are loading', async ({ page }) => {
    await page.goto('/');

    // Check for critical elements that indicate assets loaded
    const hasStyles = await page.locator('style').count() > 0;
    const hasScripts = await page.locator('script').count() > 0;

    expect(hasStyles || hasScripts).toBeTruthy();
  });

  test('API is responding', async ({ page }) => {
    const response = await page.request.get('/api/health');
    expect(response.ok()).toBeTruthy();
  });
});
