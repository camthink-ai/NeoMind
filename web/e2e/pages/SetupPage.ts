/**
 * Setup page object (first-time setup wizard)
 */

import { Page } from '@playwright/test';
import { BasePage } from './BasePage';

export class SetupPage extends BasePage {
  readonly usernameInput: string = '#setup-username, input[id*="username"]';
  readonly passwordInput: string = '#setup-password, input[id*="password"]';
  readonly submitButton: string = 'button[type="submit"]';

  constructor(page: Page) {
    super(page);
  }

  async goto(): Promise<void> {
    await this.page.goto('/setup');
    await this.waitForLoad();
  }

  async setupAdmin(username: string, password: string): Promise<void> {
    await this.waitForVisible(this.usernameInput);
    await this.fill(this.usernameInput, username);
    await this.fill(this.passwordInput, password);
    await this.click(this.submitButton);

    // Wait for completion
    await this.page.waitForURL(/\/(login|chat|dashboard)/, { timeout: 10000 });
  }
}
