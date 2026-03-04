import { test, expect, TEST_IDS } from './fixtures';

const setupRoutes = async (page: import('@playwright/test').Page) => {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
  );
};

test.describe('Cost Dashboard', () => {
  test('renders Cost Dashboard heading', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost Dashboard' })).toBeVisible();
  });

  test('renders summary cards with data', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({
        json: {
          total_cost_usd: 12.50,
          total_tasks: 8,
          avg_cost_per_task: 1.5625,
          total_input_tokens: 100000,
          total_output_tokens: 40000,
        },
      })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await expect(page.getByText('Total Spend')).toBeVisible();
    await expect(page.getByText('$12.50')).toBeVisible();
    await expect(page.getByText('Total Tasks')).toBeVisible();
    await expect(page.getByText('8')).toBeVisible();
    await expect(page.getByText('Avg Cost / Task')).toBeVisible();
    await expect(page.getByText('$1.56')).toBeVisible();
  });

  test('time range selector is visible with options', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    // The select trigger should show "Last 30 days" by default
    await expect(page.getByText('Last 30 days')).toBeVisible();
  });

  test('Daily Spend chart section renders', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 5, total_tasks: 3, avg_cost_per_task: 1.67, total_input_tokens: 50000, total_output_tokens: 20000 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) =>
      route.fulfill({
        json: [
          { day: '2025-01-01', total_input_tokens: 10000, total_output_tokens: 5000, total_compute_seconds: 100, cost_usd: 2.0, task_count: 1 },
          { day: '2025-01-02', total_input_tokens: 15000, total_output_tokens: 7000, total_compute_seconds: 150, cost_usd: 3.0, task_count: 2 },
        ],
      })
    );
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Daily Spend' })).toBeVisible();
    // SVG chart should be rendered
    await expect(page.locator('svg')).toBeVisible();
  });

  test('Cost by User table renders with data', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) =>
      route.fulfill({
        json: [
          { group_key: 'testadmin', total_input_tokens: 60000, total_output_tokens: 24000, total_compute_seconds: 420, cost_usd: 1.75 },
          { group_key: 'testmember', total_input_tokens: 35000, total_output_tokens: 17000, total_compute_seconds: 245, cost_usd: 1.07 },
        ],
      })
    );
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost by User' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testadmin' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testmember' })).toBeVisible();
    await expect(page.getByRole('cell', { name: '$1.75' })).toBeVisible();
  });

  test('Cost by Repository table renders with data', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) =>
      route.fulfill({
        json: [
          { group_key: 'testorg/testrepo', total_input_tokens: 80000, total_output_tokens: 35000, total_compute_seconds: 500, cost_usd: 2.50 },
        ],
      })
    );
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost by Repository' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testorg/testrepo' })).toBeVisible();
    await expect(page.getByRole('cell', { name: '$2.50' })).toBeVisible();
  });

  test('Budgets section renders with data', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) =>
      route.fulfill({
        json: [
          { id: 1, scope_type: 'user', scope: 'testadmin', monthly_limit_usd: 100.0, alert_threshold_pct: 80, created_by: 'testadmin', created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z' },
          { id: 2, scope_type: 'org', scope: 'testorg', monthly_limit_usd: 500.0, alert_threshold_pct: 90, created_by: 'testadmin', created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z' },
        ],
      })
    );
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Budgets' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Budget/ })).toBeVisible();
    await expect(page.getByText('testadmin')).toBeVisible();
    await expect(page.getByText('$100.00')).toBeVisible();
    await expect(page.getByText('80%')).toBeVisible();
  });

  test('Add Budget opens create dialog', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) => route.fulfill({ json: [] }));
    await page.goto('/cost');

    await page.getByRole('button', { name: /Add Budget/ }).click();

    await expect(page.getByRole('heading', { name: 'Add Budget' })).toBeVisible();
    await expect(page.getByText('Scope Type')).toBeVisible();
    await expect(page.getByLabel('Monthly Limit (USD)')).toBeVisible();
    await expect(page.getByLabel('Alert Threshold (%)')).toBeVisible();
  });

  test('delete budget shows confirmation dialog', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/cost/summary*', (route) =>
      route.fulfill({ json: { total_cost_usd: 0, total_tasks: 0, avg_cost_per_task: 0, total_input_tokens: 0, total_output_tokens: 0 } })
    );
    await page.route('**/api/admin/cost/trends*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-user*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/cost/by-repo*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/budgets', (route) =>
      route.fulfill({
        json: [
          { id: 1, scope_type: 'user', scope: 'testadmin', monthly_limit_usd: 100.0, alert_threshold_pct: 80, created_by: 'testadmin', created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z' },
        ],
      })
    );
    await page.goto('/cost');

    // Click the delete button in the budgets table
    const budgetsSection = page.locator('[class*="card"]').filter({ hasText: 'Budgets' });
    await budgetsSection.getByRole('button').filter({ has: page.locator('[class*="lucide"]') }).last().click();

    await expect(page.getByRole('heading', { name: 'Delete Budget' })).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this budget?')).toBeVisible();
  });

  test('non-admin user is redirected away from cost dashboard', async ({ memberPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.member, name: 'Test Member', role: 'member' } })
    );
    await page.goto('/cost');
    await expect(page).toHaveURL('/');
  });
});
