/**
 * E2E Test configuration and global fixtures
 */

import { test as base } from '@playwright/test';

// Extend base test with custom fixtures
export const test = base.extend<{
  // Add custom fixtures here if needed
  authenticatedPage: void;
}>('');

export const expect = base.expect;
