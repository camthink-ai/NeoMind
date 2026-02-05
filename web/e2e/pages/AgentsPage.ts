/**
 * Agents page object
 */

import { Page, expect } from '@playwright/test';
import { BasePage } from './BasePage';

export class AgentsPage extends BasePage {
  readonly agentList: string = '[data-testid="agent-list"], .agent-list';
  readonly agentCard: string = '[data-testid="agent-card"], .agent-card';
  readonly createAgentButton: string = 'button:has-text("创建"), button:has-text("Create"), [data-testid="create-agent-button"]';

  constructor(page: Page) {
    super(page);
  }

  async goto(): Promise<void> {
    await this.page.goto('/agents');
    await this.waitForLoad();
  }

  async getAgentCount(): Promise<number> {
    await this.waitForVisible(this.agentList);
    return await this.page.locator(this.agentCard).count();
  }

  async findAgentByName(name: string): Promise<boolean> {
    const agentLocator = this.page.locator(`${this.agentCard}:has-text("${name}")`);
    return await agentLocator.count() > 0;
  }

  async clickAgentAction(agentName: string, action: 'execute' | 'edit' | 'delete'): Promise<void> {
    const card = this.page.locator(`${this.agentCard}:has-text("${agentName}")`);
    const button = card.locator(`button:has-text("${action}")`);
    await button.click();
  }
}
