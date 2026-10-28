import { test, expect, TEST_IDS } from './fixtures';

const { source, issue } = TEST_IDS.sentry;

test.describe('Admin - Sentry Sources section', () => {
  test('shows seeded source with its last poll error', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    const row = section.locator('tr').filter({ hasText: source.name });

    await expect(row).toBeVisible();
    await expect(row.getByText(`${source.org}/backend, web`)).toBeVisible();
    await expect(row.getByText(source.repo)).toBeVisible();
    await expect(row.getByText('Sentry Alert')).toBeVisible();
    await expect(row.getByText('Sentry API returned 401 Unauthorized')).toBeVisible();
  });

  test('View Issues lists GitHub issues already opened', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    await section.locator('tr').filter({ hasText: source.name }).getByTitle('View Issues').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Opened Issues')).toBeVisible();
    await expect(dialog.getByRole('link', { name: issue.shortId })).toBeVisible();
    await expect(dialog.getByText(issue.title)).toBeVisible();
    await expect(dialog.getByRole('link', { name: issue.github })).toHaveAttribute(
      'href',
      'https://github.com/testorg/testrepo/issues/950',
    );
  });

  test('toggle source enabled/disabled', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    const row = section.locator('tr').filter({ hasText: source.name });

    await row.getByText('On', { exact: true }).click();
    await expect(row.getByText('Off', { exact: true })).toBeVisible({ timeout: 5000 });

    await row.getByText('Off', { exact: true }).click();
    await expect(row.getByText('On', { exact: true })).toBeVisible({ timeout: 5000 });
  });

  test('edit keeps the stored token when the field is left empty', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    const row = section.locator('tr').filter({ hasText: source.name });
    const dialog = page.getByRole('dialog');

    await row.getByTitle('Edit').click();
    await expect(dialog.getByLabel('Auth Token')).toHaveValue('');
    await dialog.getByLabel(/^Environments/).fill('production, prod');
    await dialog.getByRole('button', { name: 'Save Changes' }).click();
    await expect(dialog).not.toBeVisible();
    await expect(row.getByText('(production, prod)')).toBeVisible();

    // Restore for test isolation
    await row.getByTitle('Edit').click();
    await dialog.getByLabel(/^Environments/).fill('production');
    await dialog.getByRole('button', { name: 'Save Changes' }).click();
    await expect(dialog).not.toBeVisible();
  });

  test('create and delete a source', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    const dialog = page.getByRole('dialog');

    await section.getByRole('button', { name: /Add Sentry Source/ }).click();
    await expect(dialog.getByLabel(/^Environments/)).toHaveValue('production');
    await expect(dialog.getByLabel(/^GitHub Labels/)).toHaveValue('Sentry Alert');
    await expect(dialog.getByLabel('Run As User')).toHaveValue('testadmin');

    await dialog.getByLabel('Name').fill('Staging Errors');
    await dialog.getByLabel('Sentry Org').fill('testorg-sentry');
    await dialog.getByLabel(/^Projects/).fill('web');
    await dialog.getByLabel('Auth Token').fill('sntrys_test_token');
    await dialog.getByLabel('Target Repo').fill('testorg/frontend');
    await dialog.getByRole('button', { name: 'Create Source' }).click();
    await expect(dialog).not.toBeVisible();

    const row = section.locator('tr').filter({ hasText: 'Staging Errors' });
    await expect(row.getByText('testorg-sentry/web')).toBeVisible();
    await expect(row.getByText('Never')).toBeVisible();

    await row.getByTitle('Delete').click();
    await dialog.getByRole('button', { name: 'Delete' }).click();
    await expect(row).not.toBeVisible();
  });

  test('Test Poll dialog opens and shows Run button', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Sentry Sources' });
    await section.locator('tr').filter({ hasText: source.name }).getByTitle('Test Poll').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Nothing is created')).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Run Test Poll' })).toBeVisible();
  });
});
