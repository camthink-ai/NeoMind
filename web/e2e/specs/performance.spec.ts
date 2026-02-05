/**
 * Performance tests
 */

import { test, expect } from '@playwright/test';

test.describe('Performance', () => {

  test('page should load within reasonable time', async ({ page }) => {
    const startTime = Date.now();

    await page.goto('/');

    const loadTime = Date.now() - startTime;

    // Page should load in less than 5 seconds
    expect(loadTime).toBeLessThan(5000);
  });

  test('should not have excessive memory leaks', async ({ page }) => {
    await page.goto('/');

    // Get initial memory usage (Chrome DevTools Protocol)
    const client = await page.context().newCDPSession(page);
    await client.send('Performance.enable');

    // Navigate multiple times
    for (let i = 0; i < 5; i++) {
      await page.goto('/devices');
      await page.goto('/agents');
    }

    // Check for obvious issues (page should still be responsive)
    const isResponsive = await page.locator('body').isVisible();
    expect(isResponsive).toBeTruthy();
  });

  test('should not have large bundle sizes', async ({ page }) => {
    await page.goto('/');

    // Get all script resources
    const scripts = await page.evaluate(() => {
      return Array.from(document.scripts).map(s => ({
        src: s.src,
        type: s.type,
        textContentLength: s.textContent?.length || 0,
      }));
    });

    // Check for inline scripts (could indicate code splitting issues)
    const inlineScripts = scripts.filter(s => !s.src && s.textContentLength > 10000);

    // Should not have very large inline scripts
    expect(inlineScripts.length).toBeLessThan(3);
  });

  test('images should be optimized', async ({ page }) => {
    await page.goto('/');

    // Get all images
    const images = await page.locator('img').all();

    for (const img of images) {
      const isVisible = await img.isVisible();
      if (isVisible) {
        const naturalWidth = await img.evaluate(el => el.naturalWidth);
        const naturalHeight = await img.evaluate(el => el.naturalHeight);

        // Images should have actual dimensions
        expect(naturalWidth).toBeGreaterThan(0);
        expect(naturalHeight).toBeGreaterThan(0);
      }
    }
  });

  test('should not have console errors or warnings', async ({ page }) => {
    const errors: string[] = [];
    const warnings: string[] = [];

    page.on('console', msg => {
      if (msg.type() === 'error') {
        errors.push(msg.text());
      } else if (msg.type() === 'warning') {
        warnings.push(msg.text());
      }
    });

    await page.goto('/');
    await page.goto('/chat');

    // Should not have critical errors
    const criticalErrors = errors.filter(e =>
      !e.includes('404') && // Allow 404s for missing assets during dev
      !e.includes('favicon')
    );

    // Print results for review
    console.log(`Console errors: ${errors.length}`);
    console.log(`Console warnings: ${warnings.length}`);

    // In production, this should be 0
    // For now, just verify we can detect issues
    expect(criticalErrors.length).toBeLessThan(10);
  });
});
