/**
 * Devices page object
 */

import { Page, expect } from '@playwright/test';
import { BasePage } from './BasePage';

export class DevicesPage extends BasePage {
  readonly deviceList: string = '[data-testid="device-list"], .device-list';
  readonly deviceItem: string = '[data-testid="device-item"], .device-item';
  readonly addDeviceButton: string = 'button:has-text("添加设备"), button:has-text("Add Device"), [data-testid="add-device-button"]';

  constructor(page: Page) {
    super(page);
  }

  /**
   * Navigate to devices page
   */
  async goto(): Promise<void> {
    await this.page.goto('/devices');
    await this.waitForLoad();
  }

  /**
   * Get device count
   */
  async getDeviceCount(): Promise<number> {
    // Check if device list exists first
    const listExists = await this.page.locator(this.deviceList).count() > 0;

    if (!listExists) {
      // Device list may not exist when there are no devices
      // Return 0 instead of waiting for visible
      return await this.page.locator(this.deviceItem).count();
    }

    // List exists, wait for it to be visible
    const listVisible = await this.page.locator(this.deviceList).isVisible().catch(() => false);
    if (listVisible) {
      return await this.page.locator(this.deviceItem).count();
    }

    return 0;
  }

  /**
   * Find device by name
   */
  async findDeviceByName(name: string): Promise<boolean> {
    const deviceLocator = this.page.locator(`${this.deviceItem}:has-text("${name}")`);
    return await deviceLocator.count() > 0;
  }

  /**
   * Click device control button
   */
  async clickDeviceControl(deviceName: string, command: string): Promise<void> {
    const deviceLocator = this.page.locator(`${this.deviceItem}:has-text("${deviceName}")`);
    const controlButton = deviceLocator.locator(`button:has-text("${command}")`);

    await controlButton.click();
  }

  /**
   * Open device discovery dialog
   */
  async openDiscovery(): Promise<void> {
    const discoverButton = this.page.locator('button:has-text("发现设备"), button:has-text("Discover"), [data-testid="discover-button"]');
    await discoverButton.click();

    // Wait for dialog to open
    await this.page.waitForSelector('[role="dialog"], .dialog', { state: 'visible' });
  }

  /**
   * Verify we're on the devices page
   */
  async isVisible(): Promise<boolean> {
    // Check if we're on the devices page by URL or page element
    const url = this.page.url();
    if (url.includes('/device')) {
      return true;
    }

    // Also check if device list exists
    return await this.page.locator(this.deviceList).count() > 0;
  }
}
