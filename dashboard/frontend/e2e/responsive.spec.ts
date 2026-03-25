import { test, expect } from './fixtures';

const VIEWPORTS = {
  mobile: { width: 375, height: 667 },
  tablet: { width: 768, height: 1024 },
  desktop: { width: 1280, height: 800 },
} as const;

test.describe('Responsive - Mobile viewport', () => {
  test.use({ viewport: VIEWPORTS.mobile });

  test('home page renders without horizontal overflow', async ({ adminPage: page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const body = page.locator('body');
    const bodyBox = await body.boundingBox();
    expect(bodyBox).toBeTruthy();
    expect(bodyBox!.width).toBeLessThanOrEqual(VIEWPORTS.mobile.width + 1);
  });

  test('previews page renders at mobile width', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await expect(page.getByRole('heading', { name: 'Previews' })).toBeVisible();
    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.mobile.width + 1);
  });

  test('settings form is usable at mobile width', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    const apiKeyInput = page.locator('#api-key');
    await expect(apiKeyInput).toBeVisible();
    await apiKeyInput.fill('test-key');
    await expect(apiKeyInput).toHaveValue('test-key');
  });

  test('sidebar trigger is visible on mobile', async ({ adminPage: page }) => {
    await page.goto('/');

    const trigger = page.locator('[data-sidebar="trigger"]');
    if (await trigger.count() > 0) {
      await expect(trigger).toBeVisible();
    }
  });

  test('admin page tables are scrollable at mobile width', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Admin', exact: true })).toBeVisible();
    const scrollableDiv = page.locator('.overflow-x-auto').first();
    await expect(scrollableDiv).toBeVisible();
  });
});

test.describe('Responsive - Tablet viewport', () => {
  test.use({ viewport: VIEWPORTS.tablet });

  test('home page renders at tablet width', async ({ adminPage: page }) => {
    await page.goto('/');

    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.tablet.width + 1);
  });

  test('cost dashboard summary cards render at tablet width', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Total Spend')).toBeVisible();
    await expect(page.getByText('Total Tasks')).toBeVisible();
    await expect(page.getByText('Avg Cost / Task')).toBeVisible();
  });

  test('audit log filters render at tablet width', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await expect(page.locator('label', { hasText: 'Event Type' })).toBeVisible();
    await expect(page.getByPlaceholder('Filter by actor...')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by target...')).toBeVisible();
  });
});

test.describe('Responsive - Desktop viewport', () => {
  test.use({ viewport: VIEWPORTS.desktop });

  test('home page renders at desktop width', async ({ adminPage: page }) => {
    await page.goto('/');

    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.desktop.width + 1);
  });

  test('sidebar navigation links are visible at desktop', async ({ adminPage: page }) => {
    await page.goto('/');

    const sidebar = page.locator('[data-sidebar="sidebar"]').first();
    await expect(sidebar.getByRole('link', { name: 'Home' })).toBeVisible();
    await expect(sidebar.getByRole('link', { name: 'Previews' })).toBeVisible();
    await expect(sidebar.getByRole('link', { name: 'Tasks' })).toBeVisible();
    await expect(sidebar.getByRole('link', { name: 'Settings' })).toBeVisible();
  });

  test('admin navigation links visible for admin users at desktop', async ({ adminPage: page }) => {
    await page.goto('/');

    const sidebar = page.locator('[data-sidebar="sidebar"]').first();
    await expect(sidebar.getByRole('link', { name: 'Admin' })).toBeVisible();
    await expect(sidebar.getByRole('link', { name: 'Cost' })).toBeVisible();
    await expect(sidebar.getByRole('link', { name: 'Audit Log' })).toBeVisible();
  });

  test('cost dashboard renders all sections at desktop', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost Dashboard' })).toBeVisible();
    await expect(page.getByText('Daily Spend')).toBeVisible();
    await expect(page.getByText('Cost by User')).toBeVisible();
    await expect(page.getByText('Cost by Repository')).toBeVisible();
    await expect(page.getByText('Budgets').first()).toBeVisible();
  });
});
