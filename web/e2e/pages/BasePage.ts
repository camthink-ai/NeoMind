/**
 * Base page object with common functionality
 */

import { Page, Locator } from '@playwright/test';

export class BasePage {
  constructor(protected page: Page) {}

  /**
   * Navigate to a URL
   */
  async goto(path: string): Promise<void> {
    await this.page.goto(path);
  }

  /**
   * Wait for page to be loaded
   */
  async waitForLoad(): Promise<void> {
    // Use domcontentloaded instead of networkidle for faster tests
    // networkidle can timeout with long-running WebSocket connections
    await this.page.waitForLoadState('domcontentloaded');
    await this.page.waitForTimeout(500);
  }

  /**
   * Get current URL
   */
  getUrl(): string {
    return this.page.url();
  }

  /**
   * Wait for element to be visible
   */
  async waitForVisible(selector: string, timeout = 5000): Promise<Locator> {
    const locator = this.page.locator(selector);
    await locator.waitFor({ state: 'visible', timeout });
    return locator;
  }

  /**
   * Click element
   */
  async click(selector: string): Promise<void> {
    await this.page.click(selector);
  }

  /**
   * Fill input field (use type() for React controlled components)
   */
  async fill(selector: string, value: string): Promise<void> {
    await this.page.fill(selector, value);
  }

  /**
   * Type into input field (triggers React onChange events)
   */
  async type(selector: string, value: string): Promise<void> {
    const locator = this.page.locator(selector);
    await locator.click();
    await locator.clear();
    await locator.type(value, { delay: 10 });
  }

  /**
   * Get text content
   */
  async getText(selector: string): Promise<string> {
    return await this.page.locator(selector).textContent() || '';
  }

  /**
   * Wait for toast notification
   */
  async waitForToast(timeout = 5000): Promise<Locator> {
    return this.waitForVisible('[role="alert"], .toast, [data-testid="toast"]', timeout);
  }

  /**
   * Take screenshot
   */
  async screenshot(name: string): Promise<void> {
    await this.page.screenshot({ path: `test-results/screenshots/${name}.png` });
  }
}
