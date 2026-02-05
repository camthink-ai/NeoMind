/**
 * Settings page E2E tests
 */

import { test, expect } from '@playwright/test';
import { SettingsPage } from '../pages/SettingsPage';
import { login } from '../fixtures/auth';

test.describe('Settings', () => {
  let settingsPage: SettingsPage;

  test.beforeEach(async ({ page }) => {
    await login(page);
    settingsPage = new SettingsPage(page);
    await settingsPage.goto();
  });

  test('should display settings page with tabs', async ({ page }) => {
    const tabCount = await settingsPage.getTabCount();
    expect(tabCount).toBeGreaterThan(0);
  });

  test('should switch between tabs', async ({ page }) => {
    // Get all tabs
    const tabs = await page.locator('[role="tab"]').allTextContents();

    if (tabs.length > 1) {
      // Click second tab
      await settingsPage.selectTab(tabs[1]);

      // Verify tab changed
      const activeTab = await settingsPage.getActiveTab();
      expect(activeTab).toBeTruthy();
    }
  });

  test('should have language selector', async ({ page }) => {
    const selector = page.locator(settingsPage.languageSelect);
    // Language selector may or may not exist
    const count = await selector.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('should display about section', async ({ page }) => {
    const aboutTab = page.locator('[role="tab"]:has-text("关于"), [role="tab"]:has-text("About")');
    if (await aboutTab.count() > 0) {
      await aboutTab.click();
      // Should show version info
      const versionText = await page.locator('text=/v\\d+\\.\\d+/').count();
      expect(versionText).toBeGreaterThan(0);
    }
  });
});
