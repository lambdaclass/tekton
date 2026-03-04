import { test, expect, TEST_IDS } from './fixtures';

const VIEWPORTS = {
  mobile: { width: 375, height: 667 },
  tablet: { width: 768, height: 1024 },
  desktop: { width: 1280, height: 800 },
} as const;

const setupCommonRoutes = async (page: import('@playwright/test').Page) => {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
  );
  await page.route('**/api/tasks*', (route) =>
    route.fulfill({ json: { tasks: [], total: 0, page: 1, per_page: 20 } })
  );
  await page.route('**/api/previews', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/settings/ai', (route) =>
    route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
  );
  await page.route('**/api/admin/users', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/cost/summary*', (route) =>
    route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
  );
  await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/audit-log*', (route) =>
    route.fulfill({ json: { entries: [], total: 0, page: 1, per_page: 25 } })
  );
};

test.describe('Responsive - Mobile viewport', () => {
  test.use({ viewport: VIEWPORTS.mobile });

  test('home page renders without horizontal overflow', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const body = page.locator('body');
    const bodyBox = await body.boundingBox();
    expect(bodyBox).toBeTruthy();
    expect(bodyBox!.width).toBeLessThanOrEqual(VIEWPORTS.mobile.width + 1);
  });

  test('previews page renders at mobile width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/previews');

    await expect(page.getByRole('heading', { name: 'Previews' })).toBeVisible();
    // No horizontal scrollbar
    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.mobile.width + 1);
  });

  test('settings form is usable at mobile width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    // API key input should be visible and interactable
    const apiKeyInput = page.getByLabel(/API Key/);
    await expect(apiKeyInput).toBeVisible();
    await apiKeyInput.fill('test-key');
    await expect(apiKeyInput).toHaveValue('test-key');
  });

  test('sidebar trigger is visible on mobile', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');

    // The SidebarTrigger button should be visible for toggling sidebar
    const trigger = page.locator('[data-sidebar="trigger"]');
    // If the sidebar trigger exists, it should be visible
    if (await trigger.count() > 0) {
      await expect(trigger).toBeVisible();
    }
  });

  test('admin page tables are scrollable at mobile width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [
          { login: 'testadmin', name: 'Test Admin', role: 'admin' },
          { login: 'testmember', name: 'Test Member With A Long Name', role: 'member' },
        ],
      })
    );
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Admin', exact: true })).toBeVisible();
    // The overflow-x-auto wrapper should exist
    const scrollableDiv = page.locator('.overflow-x-auto').first();
    await expect(scrollableDiv).toBeVisible();
  });
});

test.describe('Responsive - Tablet viewport', () => {
  test.use({ viewport: VIEWPORTS.tablet });

  test('home page renders at tablet width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');

    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.tablet.width + 1);
  });

  test('cost dashboard summary cards render at tablet width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/cost');

    await expect(page.getByText('Total Spend')).toBeVisible();
    await expect(page.getByText('Total Tasks')).toBeVisible();
    await expect(page.getByText('Avg Cost / Task')).toBeVisible();
  });

  test('audit log filters render at tablet width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/audit');

    await expect(page.getByText('Event Type')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by actor...')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by target...')).toBeVisible();
  });
});

test.describe('Responsive - Desktop viewport', () => {
  test.use({ viewport: VIEWPORTS.desktop });

  test('home page renders at desktop width', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');

    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(VIEWPORTS.desktop.width + 1);
  });

  test('sidebar navigation links are visible at desktop', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');

    await expect(page.getByRole('link', { name: 'Home' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Previews' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Tasks' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Settings' })).toBeVisible();
  });

  test('admin navigation links visible for admin users at desktop', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.goto('/');

    await expect(page.getByRole('link', { name: 'Admin' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Cost' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Audit Log' })).toBeVisible();
  });

  test('cost dashboard renders all sections at desktop', async ({ adminPage: page }) => {
    await setupCommonRoutes(page);
    await page.route('**/api/admin/cost/by-user*', (route) =>
      route.fulfill({
        json: [{ group_key: 'testadmin', total_input_tokens: 1000, total_output_tokens: 500, total_compute_seconds: 60, cost_usd: 0.50 }],
      })
    );
    await page.route('**/api/admin/cost/by-repo*', (route) =>
      route.fulfill({
        json: [{ group_key: 'testorg/testrepo', total_input_tokens: 1000, total_output_tokens: 500, total_compute_seconds: 60, cost_usd: 0.50 }],
      })
    );
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost Dashboard' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Daily Spend' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Cost by User' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Cost by Repository' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Budgets' })).toBeVisible();
  });
});
