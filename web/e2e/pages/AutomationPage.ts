/**
 * Automation/Rule page object
 */

import { Page } from '@playwright/test';
import { BasePage } from './BasePage';

export class AutomationPage extends BasePage {
  readonly automationList: string = '[data-testid="automation-list"], .automation-list';
  readonly createButton: string = 'button:has-text("创建"), button:has-text("新建"), button:has-text("Create"), [data-testid="create-automation-button"]';
  readonly testButton: string = 'button:has-text("测试"), button:has-text("Test")';

  constructor(page: Page) {
    super(page);
  }

  async goto(): Promise<void> {
    await this.page.goto('/automation');
    await this.waitForLoad();
  }

  async getAutomationCount(): Promise<number> {
    const list = this.page.locator(this.automationList);
    if (await list.count() > 0) {
      return await this.page.locator('[data-testid="automation-item"], .automation-item').count();
    }
    return 0;
  }

  async openCreateDialog(): Promise<void> {
    const btn = this.page.locator(this.createButton).first();
    if (await btn.count() > 0) {
      await btn.click();
    }
  }
}
