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

test.describe.serial('Admin - Org Policy CRUD', () => {
  test('create org policy for e2e-temp-org', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await dialog.getByLabel('Organization').fill('e2e-temp-org');
    await dialog.getByLabel('Max Cost (USD)').fill('75');
    await dialog.getByLabel('Require Approval Above (USD)').fill('30');

    await dialog.getByRole('button', { name: 'Create Org Policy' }).click();

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await expect(orgSection.getByText('e2e-temp-org')).toBeVisible({ timeout: 10000 });
  });

  test('verify e2e-temp-org org policy persists', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await expect(orgSection.getByText('e2e-temp-org')).toBeVisible();
    await expect(orgSection.getByText('$75')).toBeVisible();
  });

  test('delete e2e-temp-org org policy', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    const policyRow = orgSection.locator('tr').filter({ hasText: 'e2e-temp-org' });
    await policyRow.getByRole('button').click();

    await expect(page.getByText('Delete Org Policy')).toBeVisible();
    await page.getByRole('button', { name: 'Delete', exact: true }).click();

    await expect(orgSection.getByText('e2e-temp-org')).not.toBeVisible({ timeout: 10000 });
  });

  test('verify e2e-temp-org org policy is gone', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await expect(orgSection.getByText('e2e-temp-org')).not.toBeVisible();
  });
});

test.describe('Admin - Org Policy form details', () => {
  test('Add Org Policy dialog has full form fields', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel('Organization')).toBeVisible();
    await expect(dialog.getByText('Protected Branches').first()).toBeVisible();
    await expect(dialog.getByText('Tool Restrictions', { exact: true })).toBeVisible();
    await expect(dialog.getByText('Network Egress', { exact: true })).toBeVisible();
    await expect(dialog.getByLabel('Max Cost (USD)')).toBeVisible();
    await expect(dialog.getByLabel('Require Approval Above (USD)')).toBeVisible();
  });

  test('selecting deny tool mode shows tool input', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    const toolSelect = dialog.locator('select').filter({ hasText: 'No tool restrictions' });
    await toolSelect.selectOption('deny');

    await expect(dialog.getByPlaceholder('Tool name to deny (e.g. Bash)')).toBeVisible();
  });

  test('selecting allowlist network mode shows domain input', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    const networkSelect = dialog.locator('select').filter({ hasText: 'No network restrictions' });
    await networkSelect.selectOption('allowlist');

    await expect(dialog.getByPlaceholder('Domain to allow (e.g. github.com)')).toBeVisible();
  });

  test('org policy form has preset selector', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    const presetSelect = dialog.locator('select').filter({ hasText: 'Custom (no preset)' });
    await expect(presetSelect).toBeVisible();
  });

  test('selecting org policy preset fills form fields', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Org Policy/ }).click();

    const dialog = page.getByRole('dialog');
    const presetSelect = dialog.locator('select').filter({ hasText: 'Custom (no preset)' });
    await presetSelect.selectOption('strict');

    await expect(dialog.getByLabel('Max Cost (USD)')).toHaveValue('25');
  });
});

test.describe('Admin - Policy presets', () => {
  test('selecting a preset fills form fields', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();

    const dialog = page.getByRole('dialog');
    const presetSelect = dialog.locator('select').filter({ hasText: 'Custom (no preset)' });
    await expect(presetSelect).toBeVisible();

    // Select the "strict" preset
    await presetSelect.selectOption('strict');

    // Verify preset filled in the max cost and approval threshold
    await expect(dialog.getByLabel('Max Cost (USD)')).toHaveValue('25');
    await expect(dialog.getByLabel('Require Approval Above (USD)')).toHaveValue('10');

    // Verify protected branches were filled (strict has main, master, develop, release/*)
    await expect(dialog.getByText('develop')).toBeVisible();
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
