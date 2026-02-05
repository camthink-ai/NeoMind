/**
 * Login page object
 */

import { Page, expect } from '@playwright/test';
import { BasePage } from './BasePage';

export class LoginPage extends BasePage {
  readonly usernameInput: string = '#username';
  readonly passwordInput: string = '#password';
  readonly submitButton: string = 'button[type="submit"]';

  constructor(page: Page) {
    super(page);
  }

  /**
   * Navigate to login page
   */
  async goto(): Promise<void> {
    await this.page.goto('/login');
    await this.waitForLoad();
  }

  /**
   * Login with credentials
   */
  async login(username: string, password: string): Promise<void> {
    await this.waitForVisible(this.usernameInput);
    await this.type(this.usernameInput, username);
    await this.type(this.passwordInput, password);
    await this.click(this.submitButton);

    // Wait for navigation after successful login (redirects to /)
    await this.page.waitForFunction(() => {
      const url = window.location.href;
      return !url.includes('/login');
    }, { timeout: 10000 });
  }

  /**
   * Get error message
   */
  async getErrorMessage(): Promise<string> {
    const errorLocator = this.page.locator('.text-destructive, [role="alert"]');
    if (await errorLocator.count() > 0) {
      return await errorLocator.first().textContent() || '';
    }
    return '';
  }

  /**
   * Verify login form is visible
   */
  async isVisible(): Promise<boolean> {
    return await this.page.locator(this.usernameInput).isVisible();
  }
}
