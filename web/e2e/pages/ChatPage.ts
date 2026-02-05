/**
 * Chat page object
 */

import { Page, expect } from '@playwright/test';
import { BasePage } from './BasePage';

export class ChatPage extends BasePage {
  readonly messageInput: string = 'textarea';
  // Send button - try to find the blue round button
  readonly sendButton: string = 'button:has(svg)';
  readonly messagesContainer: string = '[data-testid="messages-container"], .messages-container, .chat-messages';

  constructor(page: Page) {
    super(page);
  }

  /**
   * Navigate to chat page
   */
  async goto(): Promise<void> {
    await this.page.goto('/chat');
    await this.waitForLoad();
  }

  /**
   * Send a message
   */
  async sendMessage(message: string): Promise<void> {
    const textarea = this.page.locator(this.messageInput).first();
    await textarea.waitFor({ state: 'visible', timeout: 5000 });

    // Use type() instead of fill() to properly trigger React onChange events
    await textarea.click(); // Focus first
    await textarea.clear();
    await textarea.type(message, { delay: 10 });

    // Wait a bit for React state to update and button to enable
    await this.page.waitForTimeout(500);

    // Try to find and click send button
    const sendButton = this.page.locator('button').filter({ hasText: /^$/ }).filter({ has: this.page.locator('svg') });
    const count = await sendButton.count();

    if (count > 0) {
      await sendButton.first().click();
    } else {
      // Fallback: try to press Enter to send
      await this.page.keyboard.press('Enter');
    }

    // Wait for message to be sent
    await this.page.waitForTimeout(500);
  }

  /**
   * Wait for AI response
   */
  async waitForResponse(timeout = 30000): Promise<string> {
    const responseLocator = this.page.locator('.message.assistant, [data-testid="assistant-message"]').last();
    await responseLocator.waitFor({ state: 'visible', timeout });
    return await responseLocator.textContent() || '';
  }

  /**
   * Get all user messages
   */
  async getUserMessages(): Promise<string[]> {
    const messages = await this.page.locator('.message.user, [data-testid="user-message"]').allTextContents();
    return messages;
  }

  /**
   * Get all assistant messages
   */
  async getAssistantMessages(): Promise<string[]> {
    const messages = await this.page.locator('.message.assistant, [data-testid="assistant-message"]').allTextContents();
    return messages;
  }

  /**
   * Create a new session
   */
  async newSession(): Promise<void> {
    const newSessionButton = this.page.locator('button:has-text("新对话"), button:has-text("New Chat"), [data-testid="new-session-button"]');
    if (await newSessionButton.count() > 0) {
      await newSessionButton.first().click();
      await this.page.waitForTimeout(500);
    }
  }

  /**
   * Verify we're on the chat page
   */
  async isVisible(): Promise<boolean> {
    return await this.page.locator(this.messageInput).isVisible();
  }
}
