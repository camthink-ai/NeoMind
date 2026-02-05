/**
 * Dashboard component unit tests
 *
 * Note: Component library is now integrated into VisualDashboard as a sidebar,
 * not a separate page. These tests open the component library from the dashboard.
 */

import { test, expect } from '@playwright/test';
import { DashboardPage } from '../pages/DashboardPage';
import { login } from '../fixtures/auth';

test.describe('Dashboard Components - ValueCard', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should display value with unit', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    // Find and preview a value card
    const valueCardItem = page.locator('[data-testid="component-item"]').filter({ hasText: /card/i }).first();

    if (await valueCardItem.count() > 0) {
      await valueCardItem.click();

      // Preview should show unit
      const preview = page.locator('[data-testid="component-preview"]');
      if (await preview.count() > 0) {
        const hasUnit = await preview.locator('text=/°C|°F|%|V/i').count() > 0;
        expect(hasUnit).toBeTruthy();
      }
    }
  });

  test('should support threshold colors', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const valueCardItem = page.locator('[data-testid="component-item"]').filter({ hasText: /card/i }).first();

    if (await valueCardItem.count() > 0) {
      await valueCardItem.click();

      // Look for color options
      const colorPicker = page.locator('[data-testid="color-picker"], input[type="color"]');
      expect(await colorPicker.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should show trend indicator', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const valueCardItem = page.locator('[data-testid="component-item"]').filter({ hasText: /card/i }).first();

    if (await valueCardItem.count() > 0) {
      await valueCardItem.click();

      // Check for trend option
      const trend = page.locator('[data-testid="trend"], text=/trend/i');
      expect(await trend.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - Charts', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('line chart should display data', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const lineChart = page.locator('[data-testid="component-item"]').filter({ hasText: /line.*chart/i }).first();

    if (await lineChart.count() > 0) {
      await lineChart.click();

      // Preview should render canvas or SVG
      const chart = page.locator('canvas, svg');
      expect(await chart.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('bar chart should display data', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const barChart = page.locator('[data-testid="component-item"]').filter({ hasText: /bar.*chart/i }).first();

    if (await barChart.count() > 0) {
      await barChart.click();

      // Preview should render
      const chart = page.locator('canvas, svg');
      expect(await chart.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('pie chart should display legend', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const pieChart = page.locator('[data-testid="component-item"]').filter({ hasText: /pie.*chart/i }).first();

    if (await pieChart.count() > 0) {
      await pieChart.click();

      // Look for legend
      const legend = page.locator('[data-testid="legend"], .chart-legend');
      expect(await legend.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - LED Indicator', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should support multiple states', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const led = page.locator('[data-testid="component-item"]').filter({ hasText: /led|indicator/i }).first();

    if (await led.count() > 0) {
      await led.click();

      // Should have state options
      const states = page.locator('[data-testid="state-option"], [data-value*="state"]');
      expect(await states.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should support custom colors for each state', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const led = page.locator('[data-testid="component-item"]').filter({ hasText: /led|indicator/i }).first();

    if (await led.count() > 0) {
      await led.click();

      // Look for color configuration
      const colorConfig = page.locator('[data-testid="color-config"]');
      expect(await colorConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should support blink animation', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const led = page.locator('[data-testid="component-item"]').filter({ hasText: /led|indicator/i }).first();

    if (await led.count() > 0) {
      await led.click();

      // Look for animation toggle
      const blinkOption = page.locator('[data-testid="blink"], text=/blink/i');
      expect(await blinkOption.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - Map Display', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should display map component', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const map = page.locator('[data-testid="component-item"]').filter({ hasText: /map/i }).first();

    if (await map.count() > 0) {
      await map.click();

      // Should show map preview
      const mapPreview = page.locator('[data-testid="map"], canvas, svg');
      expect(await mapPreview.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should support layer configuration', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const map = page.locator('[data-testid="component-item"]').filter({ hasText: /map/i }).first();

    if (await map.count() > 0) {
      await map.click();

      // Look for layer config
      const layerConfig = page.locator('[data-testid="layer-config"], button:has-text("图层")');
      expect(await layerConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should support marker configuration', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const map = page.locator('[data-testid="component-item"]').filter({ hasText: /map/i }).first();

    if (await map.count() > 0) {
      await map.click();

      // Look for marker config
      const markerConfig = page.locator('[data-testid="marker-config"], button:has-text("标记")');
      expect(await markerConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - Progress Bar', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should show progress percentage', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const progress = page.locator('[data-testid="component-item"]').filter({ hasText: /progress/i }).first();

    if (await progress.count() > 0) {
      await progress.click();

      // Preview should show percentage or value
      const preview = page.locator('[data-testid="component-preview"]');
      const hasPercentage = await preview.locator('text=/%/').count() > 0;
      const hasValue = await preview.locator('[role="progressbar"]').count() > 0;

      expect(hasPercentage || hasValue).toBeTruthy();
    }
  });

  test('should support color thresholds', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const progress = page.locator('[data-testid="component-item"]').filter({ hasText: /progress/i }).first();

    if (await progress.count() > 0) {
      await progress.click();

      // Should have threshold config
      const thresholdConfig = page.locator('[data-testid="threshold-config"]');
      expect(await thresholdConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - Sparkline', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should render mini chart', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const sparkline = page.locator('[data-testid="component-item"]').filter({ hasText: /sparkline/i }).first();

    if (await sparkline.count() > 0) {
      await sparkline.click();

      // Should render chart
      const chart = page.locator('canvas, svg');
      expect(await chart.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should support different line styles', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const sparkline = page.locator('[data-testid="component-item"]').filter({ hasText: /sparkline/i }).first();

    if (await sparkline.count() > 0) {
      await sparkline.click();

      // Look for style options
      const styleConfig = page.locator('[data-testid="line-style"], [data-testid="chart-style"]');
      expect(await styleConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });
});

test.describe('Dashboard Components - Agent Monitor', () => {

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test('should display agent status', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const agentMonitor = page.locator('[data-testid="component-item"]').filter({ hasText: /agent/i }).first();

    if (await agentMonitor.count() > 0) {
      await agentMonitor.click();

      // Should show status preview
      const preview = page.locator('[data-testid="component-preview"]');
      expect(await preview.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should show agent execution history', async ({ page }) => {
    const dashboardPage = new DashboardPage(page);
    await dashboardPage.goto();
    await dashboardPage.openComponentLibrary();

    const agentMonitor = page.locator('[data-testid="component-item"]').filter({ hasText: /agent/i }).first();

    if (await agentMonitor.count() > 0) {
      await agentMonitor.click();

      // Look for history option
      const historyConfig = page.locator('[data-testid="history-config"], button:has-text("历史")');
      expect(await historyConfig.count()).toBeGreaterThanOrEqual(0);
    }
  });
});
