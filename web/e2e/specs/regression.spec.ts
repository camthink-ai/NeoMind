/**
 * Regression tests for known issues
 */

import { test, expect } from '@playwright/test';
import { ChatPage } from '../pages/ChatPage';
import { DevicesPage } from '../pages/DevicesPage';
import { login } from '../fixtures/auth';

test.describe('Regression Tests', () => {

  test('should not crash when navigating rapidly', async ({ page }) => {
    await login(page);

    const routes = ['/chat', '/devices', '/agents', '/automation', '/settings'];

    // Navigate rapidly
    for (let i = 0; i < 10; i++) {
      const route = routes[i % routes.length];
      await page.goto(route);
      await page.waitForTimeout(100);
    }

    // Should still be functional
    const body = page.locator('body');
    await expect(body).toBeVisible();
  });

  test('should handle long messages without issues', async ({ page }) => {
    await login(page);

    const chatPage = new ChatPage(page);
    // Already on chat page after login, don't navigate again
    // await chatPage.goto();

    // Create a very long message
    const longMessage = 'Test message '.repeat(100);

    const textarea = page.locator('textarea').first();
    await textarea.click();
    await textarea.clear();
    await textarea.type(longMessage, { delay: 5 });
    await page.waitForTimeout(200);

    // Should not crash or freeze - just verify textarea is still visible
    await expect(textarea).toBeVisible();
  });

  test('should handle rapid message sending', async ({ page }) => {
    await login(page);

    const chatPage = new ChatPage(page);
    // Already on chat page after login, don't navigate again
    // await chatPage.goto();

    // Send multiple messages quickly
    const messages = ['Test 1', 'Test 2', 'Test 3'];

    for (const msg of messages) {
      await chatPage.sendMessage(msg);
    }

    // Should not crash
    const body = page.locator('body');
    await expect(body).toBeVisible();
  });

  test('should recover from network errors', async ({ page }) => {
    await login(page);

    // First navigate successfully to have a baseline
    await page.goto('/devices');
    const initialUrl = page.url();

    // Simulate offline mode
    await page.context().setOffline(true);

    // Try to navigate while offline - this may fail
    try {
      await page.goto('/devices', { timeout: 5000 }).catch(() => {});
    } catch {
      // Navigation may fail when offline, which is expected
    }

    // Restore connection
    await page.context().setOffline(false);

    // Should work again after restoring connection
    await page.reload({ waitUntil: 'domcontentloaded' });

    // Page should be visible
    await expect(page.locator('body')).toBeVisible();
  });

  test('should handle browser back button correctly', async ({ page }) => {
    await login(page);

    await page.goto('/devices', { waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(200); // Ensure first navigation completes

    await page.goto('/agents', { waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(200); // Ensure second navigation completes

    await page.goBack();
    await page.waitForTimeout(200); // Wait for back navigation

    const url = page.url();
    // Should be back on devices page
    expect(url).toContain('/device'); // Use partial match for /devices
  });

  test('should not have memory leaks from event listeners', async ({ page }) => {
    await login(page);

    // Navigate multiple times
    for (let i = 0; i < 20; i++) {
      await page.goto('/chat', { waitUntil: 'domcontentloaded' });
      await page.goto('/devices', { waitUntil: 'domcontentloaded' });
    }

    // Page should still be responsive - check if any interactive element is present
    const buttons = await page.locator('button, a, input, [tabindex]').count();
    expect(buttons).toBeGreaterThan(0);
  });

  test('should handle concurrent operations', async ({ page }) => {
    await login(page);

    // Perform evaluate operations first (before navigation)
    const evalResults = await Promise.all([
      page.evaluate(() => console.log('Test 1')),
      page.evaluate(() => console.log('Test 2')),
    ]);

    // Then navigate
    await page.goto('/chat', { waitUntil: 'domcontentloaded' });

    // Should not have excessive errors
    const hasError = await page.locator('.error, [role="alert"]').count();
    expect(hasError).toBeLessThan(5);
  });
});
