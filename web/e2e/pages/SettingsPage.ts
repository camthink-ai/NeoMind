/**
 * Settings page object
 */

import { Page } from '@playwright/test';
import { BasePage } from './BasePage';

export class SettingsPage extends BasePage {
  readonly tabs: string = '[role="tab"]';
  readonly languageSelect: string = '[data-testid="language-select"]';
  readonly timezoneSelect: string = '[data-testid="timezone-select"]';

  constructor(page: Page) {
    super(page);
  }

  async goto(): Promise<void> {
    await this.page.goto('/settings');
    await this.waitForLoad();
  }

  async getTabCount(): Promise<number> {
    return await this.page.locator(this.tabs).count();
  }

  async selectTab(tabName: string): Promise<void> {
    const tab = this.page.locator(`${this.tabs}:has-text("${tabName}")`);
    if (await tab.count() > 0) {
      await tab.click();
    }
  }

  async getActiveTab(): Promise<string> {
    const activeTab = this.page.locator(`${this.tabs}[aria-selected="true"]`);
    return await activeTab.textContent() || '';
  }
}
