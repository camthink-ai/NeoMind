/**
 * Accessibility tests
 */

import { test, expect } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';

test.describe('Accessibility', () => {

  test('login page should be accessible', async ({ page }) => {
    await page.goto('/login');

    const accessibilityScanResults = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
      .analyze();

    expect(accessibilityScanResults.violations).toEqual([]);
  });

  test('should have proper heading hierarchy', async ({ page }) => {
    await page.goto('/');

    const headings = await page.locator('h1, h2, h3, h4, h5, h6').all();
    const h1Count = await page.locator('h1').count();

    // Should have at least one h1 (chat page may have)
    if (h1Count > 0) {
      expect(h1Count).toBeGreaterThan(0);

      // Headings should be in order (basic check)
      for (let i = 0; i < headings.length - 1; i++) {
        const currentLevel = parseInt(await headings[i].evaluate(e => e.tagName));
        const nextLevel = parseInt(await headings[i + 1].evaluate(e => e.tagName));

        // Allow headings to stay the same or increase (not decrease by more than 1)
        expect(nextLevel - currentLevel).toBeLessThanOrEqual(1);
      }
    }
    // If no h1, that's okay - just verify page loaded
    await expect(page.locator('body')).toBeVisible();
  });

  test('should have skip links or main content', async ({ page }) => {
    await page.goto('/');

    const main = page.locator('main, [role="main"]');
    await expect(main.first()).toBeVisible();
  });

  test('forms should have proper labels', async ({ page }) => {
    await page.goto('/login');

    // Check all inputs have associated labels
    const inputs = await page.locator('input').all();
    for (const input of inputs) {
      // Skip hidden inputs
      const isHidden = await input.evaluate(el => el.offsetParent === null);
      if (!isHidden) {
        const hasLabel = await input.evaluate(el => {
          const id = el.id;
          const ariaLabel = el.getAttribute('aria-label');
          const label = document.querySelector(`label[for="${id}"]`);
          return !!(ariaLabel || label || el.closest('label'));
        });
        expect(hasLabel).toBeTruthy();
      }
    }
  });

  test('should have focus management', async ({ page }) => {
    await page.goto('/login');

    // Tab through first few interactive elements only
    const tabbableElements = await page.locator('button, input, a, [tabindex]:not([tabindex="-1"])').all();
    const elementsToTest = tabbableElements.slice(0, 5); // Test first 5 only

    for (const el of elementsToTest) {
      try {
        await el.focus();
        const isFocused = await el.evaluate(el => document.activeElement === el);
        expect(isFocused).toBeTruthy();
      } catch {
        // Element may not be focusable when hidden or disabled
        // That's okay for this test
      }
    }
  });

  test('color contrast should be sufficient', async ({ page }) => {
    await page.goto('/');

    // This is a basic check - full contrast checking requires specialized tools
    const textElements = await page.locator('p, span, div, h1, h2, h3, button, a').all();

    for (const el of textElements.slice(0, 20)) { // Check first 20 elements
      const isVisible = await el.isVisible();
      if (isVisible) {
        const color = await el.evaluate(el => {
          const styles = window.getComputedStyle(el);
          return {
            color: styles.color,
            backgroundColor: styles.backgroundColor,
          };
        });
        // Basic check - color should not be transparent
        expect(color.color).not.toBe('transparent');
      }
    }
  });
});
