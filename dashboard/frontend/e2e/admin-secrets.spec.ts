import { test, expect } from './fixtures';

test.describe('Admin - Secrets section', () => {
  test('renders Secrets heading and Add Secret button', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByText('Secrets').first()).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Secret/ })).toBeVisible();
  });

  test('renders secrets table with seeded data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'NPM_TOKEN' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'DEPLOY_KEY' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testorg/testrepo' }).first()).toBeVisible();
  });

  test('secret values are not displayed in the table', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByText('encrypted:fake_npm_token_value')).not.toBeVisible();
  });

  test('Add Secret button opens create dialog', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();

    await expect(page.getByRole('heading', { name: 'Add Secret' })).toBeVisible();
    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByLabel('Secret Name')).toBeVisible();
    await expect(page.getByLabel('Value')).toBeVisible();
  });

  test('secret value input is password type', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();

    const valueInput = page.getByLabel('Value');
    await expect(valueInput).toHaveAttribute('type', 'password');
  });

  test('delete secret shows confirmation dialog', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const secretsSection = page.locator('[class*="card"]').filter({ hasText: 'Secrets' });
    await secretsSection.locator('tbody tr').first().getByRole('button').click();

    await expect(page.getByText('Delete Secret')).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this secret?')).toBeVisible();
  });
});

test.describe.serial('Admin - Secrets CRUD', () => {
  test('create E2E_TEST_SECRET', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();
    await page.getByLabel('Repository').fill('testorg/testrepo');
    await page.getByLabel('Secret Name').fill('E2E_TEST_SECRET');
    await page.getByLabel('Value').fill('e2e-secret-value');
    await page.getByRole('button', { name: 'Create Secret' }).click();

    // Wait for table to update with the new secret
    await expect(page.getByRole('cell', { name: 'E2E_TEST_SECRET' })).toBeVisible({ timeout: 10000 });
  });

  test('verify E2E_TEST_SECRET appears in table on fresh load', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'E2E_TEST_SECRET' })).toBeVisible();
  });

  test('delete E2E_TEST_SECRET', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const secretsSection = page.locator('[class*="card"]').filter({ hasText: 'Secrets' });
    const secretRow = secretsSection.locator('tr').filter({ hasText: 'E2E_TEST_SECRET' });
    await secretRow.getByRole('button').click();

    // Confirm deletion in dialog
    await expect(page.getByText('Delete Secret')).toBeVisible();
    await page.getByRole('button', { name: 'Delete', exact: true }).click();

    // Verify it's gone
    await expect(page.getByRole('cell', { name: 'E2E_TEST_SECRET' })).not.toBeVisible({ timeout: 10000 });
  });

  test('verify E2E_TEST_SECRET is gone on fresh load', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'E2E_TEST_SECRET' })).not.toBeVisible();
  });
});
