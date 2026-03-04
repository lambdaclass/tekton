import { test, expect } from './fixtures';

test.describe('Admin - Repo Policies section', () => {
  test('renders Repo Policies heading and Add Policy button', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByText('Repo Policies')).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Policy/ }).first()).toBeVisible();
  });

  test('renders seeded repo policy data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'testorg/testrepo' }).first()).toBeVisible();
    await expect(page.getByText('$10')).toBeVisible();
    await expect(page.getByText('main').first()).toBeVisible();
  });

  test('Add Policy opens create dialog with form fields', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel('Repository')).toBeVisible();
    await expect(dialog.getByText('Protected Branches').first()).toBeVisible();
    await expect(dialog.getByLabel('Max Cost (USD)')).toBeVisible();
    await expect(dialog.getByLabel('Require Approval Above (USD)')).toBeVisible();
  });

  test('delete policy shows confirmation dialog and can be dismissed', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const policiesSection = page.locator('[class*="card"]').filter({ hasText: 'Repo Policies' });
    await policiesSection.locator('tbody tr').first().getByRole('button').click();

    await expect(page.getByText('Delete Policy')).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this policy?')).toBeVisible();

    // Dismiss without confirming
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByText('Are you sure you want to delete this policy?')).not.toBeVisible();
  });

  test('preset selector is available in create dialog', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();

    const presetSelect = page.locator('select').filter({ hasText: 'Custom (no preset)' });
    await expect(presetSelect).toBeVisible();
  });
});

test.describe('Admin - Org Policies section', () => {
  test('renders Org Policies heading and Add Org Policy button', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByText('Org Policies').first()).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Org Policy/ })).toBeVisible();
  });

  test('shows org policy description text', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(
      page.getByText('Org policies apply as defaults to all repositories under the organization')
    ).toBeVisible();
  });

  test('renders seeded org policy data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await expect(orgSection.getByText('testorg')).toBeVisible();
    await expect(orgSection.getByText('$50')).toBeVisible();
  });

  test('delete org policy shows confirmation dialog', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await orgSection.locator('tbody tr').first().getByRole('button').click();

    await expect(page.getByText('Delete Org Policy')).toBeVisible();
  });
});

test.describe.serial('Admin - Repo Policy CRUD', () => {
  test('create policy for testorg/e2e-temp-repo', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();
    await page.getByLabel('Repository').fill('testorg/e2e-temp-repo');
    await page.getByRole('button', { name: 'Create Policy' }).click();

    // Verify it appears in the table
    await expect(page.getByRole('cell', { name: 'testorg/e2e-temp-repo' })).toBeVisible({ timeout: 10000 });
  });

  test('verify testorg/e2e-temp-repo policy persists', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'testorg/e2e-temp-repo' })).toBeVisible();
  });

  test('delete testorg/e2e-temp-repo policy', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const policiesSection = page.locator('[class*="card"]').filter({ hasText: 'Repo Policies' });
    const policyRow = policiesSection.locator('tr').filter({ hasText: 'testorg/e2e-temp-repo' });
    await policyRow.getByRole('button').click();

    await expect(page.getByText('Delete Policy')).toBeVisible();
    await page.getByRole('button', { name: 'Delete', exact: true }).click();

    await expect(page.getByRole('cell', { name: 'testorg/e2e-temp-repo' })).not.toBeVisible({ timeout: 10000 });
  });

  test('verify testorg/e2e-temp-repo policy is gone', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'testorg/e2e-temp-repo' })).not.toBeVisible();
  });
});
