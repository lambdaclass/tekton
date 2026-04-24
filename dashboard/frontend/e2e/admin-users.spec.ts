import { test, expect } from './fixtures';

test.describe('Admin - Users section', () => {
  test('renders Admin heading and Users section', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Admin', exact: true })).toBeVisible();
    await expect(page.locator('[data-slot="card-title"]').filter({ hasText: /^\s*Users\s*$/ })).toBeVisible();
  });

  test('user list shows login, name, and role columns', async ({ adminPage: page }) => {
    await page.goto('/admin');

    // Table headers
    await expect(page.getByRole('columnheader', { name: 'Login' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Name' }).first()).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Role' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Repos' })).toBeVisible();

    // Seeded user data (scoped to the Users card specifically — filter by the
    // card title being exactly "Users" so we don't accidentally match the
    // Usage Metrics card which also contains the word "Users")
    const usersSection = page
      .locator('[class*="card"]')
      .filter({ has: page.locator('[data-slot="card-title"]', { hasText: /^\s*Users\s*$/ }) });
    await expect(usersSection.getByRole('cell', { name: 'testadmin' })).toBeVisible();
    await expect(usersSection.getByRole('cell', { name: 'Test Admin' })).toBeVisible();
    await expect(usersSection.getByRole('cell', { name: 'testmember' })).toBeVisible();
    await expect(usersSection.getByRole('cell', { name: 'Test Member' })).toBeVisible();
    await expect(usersSection.getByRole('cell', { name: 'testviewer' })).toBeVisible();
    await expect(usersSection.getByRole('cell', { name: 'Test Viewer' })).toBeVisible();
  });

  test('role select dropdown has admin, member, viewer options', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const select = page.locator('select').first();
    await expect(select).toBeVisible();

    const options = select.locator('option');
    await expect(options).toHaveCount(3);
    await expect(options.nth(0)).toHaveText('admin');
    await expect(options.nth(1)).toHaveText('member');
    await expect(options.nth(2)).toHaveText('viewer');
  });

  test('Manage button opens repo permissions dialog', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testmemberRow = page.locator('tr').filter({ hasText: 'testmember' });
    await testmemberRow.getByRole('button', { name: /Manage/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Repo permissions for testmember')).toBeVisible();
    await expect(dialog.getByText('testorg/testrepo')).toBeVisible();
    await expect(dialog.getByText('testorg/frontend')).toBeVisible();
  });

  test('non-admin user is redirected away from admin page', async ({ memberPage: page }) => {
    await page.goto('/admin');
    await expect(page).toHaveURL('/');
  });
});

test.describe.serial('Admin - User repo permissions modification', () => {
  test('add repo to testviewer', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testviewerRow = page.locator('tr').filter({ hasText: 'testviewer' });
    await testviewerRow.getByRole('button', { name: /Manage/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Repo permissions for testviewer')).toBeVisible();

    // Add a new repo
    await dialog.getByPlaceholder('owner/repo').fill('testorg/e2e-temp-repo');
    await dialog.getByPlaceholder('owner/repo').press('Enter');

    // Verify it appears as a badge
    await expect(dialog.getByText('testorg/e2e-temp-repo')).toBeVisible({ timeout: 5000 });
  });

  test('verify added repo persists for testviewer', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testviewerRow = page.locator('tr').filter({ hasText: 'testviewer' });
    await testviewerRow.getByRole('button', { name: /Manage/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('testorg/e2e-temp-repo')).toBeVisible();
  });

  test('remove added repo from testviewer', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testviewerRow = page.locator('tr').filter({ hasText: 'testviewer' });
    await testviewerRow.getByRole('button', { name: /Manage/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('testorg/e2e-temp-repo')).toBeVisible();

    // Click the X button on the e2e-temp-repo badge
    const badge = dialog.locator('span').filter({ hasText: 'testorg/e2e-temp-repo' });
    await badge.locator('button').click();

    // Verify it's removed
    await expect(dialog.getByText('testorg/e2e-temp-repo')).not.toBeVisible({ timeout: 5000 });
  });

  test('verify removed repo does not persist', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testviewerRow = page.locator('tr').filter({ hasText: 'testviewer' });
    await testviewerRow.getByRole('button', { name: /Manage/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('testorg/e2e-temp-repo')).not.toBeVisible();
  });
});

test.describe.serial('Admin - Users role change', () => {
  test('change testmember role to viewer', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testmemberRow = page.locator('tr').filter({ hasText: 'testmember' });
    const select = testmemberRow.locator('select');
    await select.selectOption('viewer');

    // Wait for the API call to complete and UI to update
    await expect(select).toHaveValue('viewer');
  });

  test('verify testmember role persisted as viewer', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testmemberRow = page.locator('tr').filter({ hasText: 'testmember' });
    const select = testmemberRow.locator('select');
    await expect(select).toHaveValue('viewer');
  });

  test('restore testmember role to member', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const testmemberRow = page.locator('tr').filter({ hasText: 'testmember' });
    const select = testmemberRow.locator('select');
    await select.selectOption('member');

    await expect(select).toHaveValue('member');
  });
});
