import { test, expect, TEST_IDS } from './fixtures';

test.describe('Cost Dashboard', () => {
  test('renders Cost Dashboard heading', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByRole('heading', { name: 'Cost Dashboard' })).toBeVisible();
  });

  test('renders summary cards with labels', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Total Spend')).toBeVisible();
    await expect(page.getByText('Total Tasks')).toBeVisible();
    await expect(page.getByText('Avg Cost / Task')).toBeVisible();
  });

  test('time range selector is visible', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Last 30 days')).toBeVisible();
  });

  test('Daily Spend chart section renders', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Daily Spend')).toBeVisible();
  });

  test('Cost by User table renders with seeded data', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Cost by User')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testadmin' }).first()).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testmember' }).first()).toBeVisible();
  });

  test('Cost by Repository table renders with seeded data', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Cost by Repository')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testorg/testrepo' })).toBeVisible();
  });

  test('Budgets section renders with seeded data', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('Budgets').first()).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Budget/ })).toBeVisible();
    await expect(page.getByText('testadmin').first()).toBeVisible();
    await expect(page.getByText('$100.00')).toBeVisible();
    await expect(page.getByText('80%')).toBeVisible();
  });

  test('Add Budget opens create dialog', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await page.getByRole('button', { name: /Add Budget/ }).click();

    await expect(page.getByText('Add Budget').first()).toBeVisible();
    await expect(page.getByText('Scope Type')).toBeVisible();
    await expect(page.getByLabel('Monthly Limit (USD)')).toBeVisible();
    await expect(page.getByLabel('Alert Threshold (%)')).toBeVisible();
  });

  test('delete budget shows confirmation dialog', async ({ adminPage: page }) => {
    await page.goto('/cost');

    const budgetsSection = page.locator('[class*="card"]').filter({ hasText: 'Budgets' });
    await budgetsSection.locator('tbody tr').first().getByRole('button').click();

    await expect(page.getByText('Delete Budget')).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this budget?')).toBeVisible();
  });

  test('switching time range to 7 days updates data', async ({ adminPage: page }) => {
    await page.goto('/cost');

    // Click the shadcn Select trigger (shows "Last 30 days" by default)
    await page.getByRole('combobox').click();

    // Select "Last 7 days" option from the popover
    await page.getByRole('option', { name: 'Last 7 days' }).click();

    // Verify the trigger now shows "Last 7 days"
    await expect(page.getByRole('combobox')).toHaveText('Last 7 days');

    // Data sections should still render (with potentially different values)
    await expect(page.getByText('Total Spend')).toBeVisible();
    await expect(page.getByText('Daily Spend')).toBeVisible();
  });

  test('switching time range to 90 days updates data', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await page.getByRole('combobox').click();
    await page.getByRole('option', { name: 'Last 90 days' }).click();

    await expect(page.getByRole('combobox')).toHaveText('Last 90 days');
    await expect(page.getByText('Total Spend')).toBeVisible();
  });

  test('non-admin user is redirected away from cost dashboard', async ({ memberPage: page }) => {
    await page.goto('/cost');
    await expect(page).toHaveURL('/');
  });
});

test.describe.serial('Cost Dashboard - Budget CRUD', () => {
  test('create budget for e2e-temp-user', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await page.getByRole('button', { name: /Add Budget/ }).click();
    await page.getByLabel('Username').fill('e2e-temp-user');
    await page.getByLabel('Monthly Limit (USD)').fill('50');
    await page.getByLabel('Alert Threshold (%)').fill('75');
    await page.getByRole('button', { name: 'Create Budget' }).click();

    // Verify it appears
    await expect(page.getByText('e2e-temp-user')).toBeVisible({ timeout: 10000 });
  });

  test('verify e2e-temp-user budget persists', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('e2e-temp-user')).toBeVisible();
    await expect(page.getByText('$50.00')).toBeVisible();
  });

  test('delete e2e-temp-user budget', async ({ adminPage: page }) => {
    await page.goto('/cost');

    const budgetsSection = page.locator('[class*="card"]').filter({ hasText: 'Budgets' });
    const budgetRow = budgetsSection.locator('tr').filter({ hasText: 'e2e-temp-user' });
    await budgetRow.getByRole('button').click();

    await expect(page.getByText('Delete Budget')).toBeVisible();
    await page.getByRole('button', { name: 'Delete', exact: true }).click();

    await expect(page.getByText('e2e-temp-user')).not.toBeVisible({ timeout: 10000 });
  });

  test('verify e2e-temp-user budget is gone', async ({ adminPage: page }) => {
    await page.goto('/cost');

    await expect(page.getByText('e2e-temp-user')).not.toBeVisible();
  });
});
