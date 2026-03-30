import { test, expect, TEST_IDS } from './fixtures';

const SOURCES = TEST_IDS.intake.sources;

test.describe('Admin - Intake Sources section', () => {
  test('renders Intake Sources section', async ({ adminPage: page }) => {
    await page.goto('/admin');

    await expect(
      page.locator('[data-slot="card-title"]').filter({ hasText: 'Intake Sources' })
    ).toBeVisible();
  });

  test('shows seeded source data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    await expect(section.getByText(SOURCES.github.name)).toBeVisible();
    await expect(section.getByText('github', { exact: true }).first()).toBeVisible();
    await expect(section.getByText(SOURCES.github.repo)).toBeVisible();
    await expect(section.getByText('120')).toBeVisible();

    await expect(section.getByText(SOURCES.linear.name)).toBeVisible();
    await expect(section.getByText('linear', { exact: true }).first()).toBeVisible();
    await expect(section.getByText(SOURCES.linear.repo)).toBeVisible();
  });

  test('enabled/disabled badges are visible', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    await expect(section.getByText('On', { exact: true }).first()).toBeVisible();
    await expect(section.getByText('Off', { exact: true }).first()).toBeVisible();
  });

  test('Add Source button is visible', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    await expect(section.getByRole('button', { name: /Add Source/ })).toBeVisible();
  });

  test('View Issues dialog shows issue data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    // Click the eye icon (View Issues) for the first source
    await section.getByTitle('View Issues').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Intake Issues')).toBeVisible();
    // Should show issues from the GitHub Bugs source
    await expect(dialog.getByText('Fix null pointer in auth module')).toBeVisible();
  });

  test('View Logs dialog shows poll log data', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    // Click the scroll icon (View Logs) for the first source
    await section.getByTitle('View Logs').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Poll Logs')).toBeVisible();
    await expect(dialog.getByText('Rate limit exceeded, partial results')).toBeVisible();
  });

  test('toggle source enabled/disabled', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    // The GitHub Bugs source starts enabled (On) — click to toggle off
    const githubRow = section.locator('tr').filter({ hasText: SOURCES.github.name });
    const badge = githubRow.getByText('On', { exact: true });
    await badge.click();

    // Should now show Off
    await expect(githubRow.getByText('Off', { exact: true })).toBeVisible({ timeout: 5000 });

    // Toggle back on for test isolation
    await githubRow.getByText('Off', { exact: true }).click();
    await expect(githubRow.getByText('On', { exact: true })).toBeVisible({ timeout: 5000 });
  });

  test('Test Poll dialog opens and shows Run button', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });

    // Click the beaker icon (Test Poll) for the first source
    await section.getByTitle('Test Poll').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('heading', { name: 'Test Poll' })).toBeVisible();
    await expect(dialog.getByText('Dry-run poll')).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Run Test Poll' })).toBeVisible();

    // Close without running
    await dialog.getByTestId('dialog-footer').getByRole('button', { name: 'Close' }).click();
    await expect(dialog).not.toBeVisible();
  });

  test('View Issues dialog close button works', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await section.getByTitle('View Issues').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    await dialog.getByTestId('dialog-footer').getByRole('button', { name: 'Close' }).click();
    await expect(dialog).not.toBeVisible();
  });

  test('View Logs dialog close button works', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await section.getByTitle('View Logs').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    await dialog.getByTestId('dialog-footer').getByRole('button', { name: 'Close' }).click();
    await expect(dialog).not.toBeVisible();
  });

  test('View Issues dialog shows status badges and task IDs', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    const githubRow = section.locator('tr').filter({ hasText: SOURCES.github.name });
    await githubRow.getByTitle('View Issues').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // Issues should show status badges
    await expect(dialog.getByText('backlog').first()).toBeVisible();
    // At least one issue should have a linked task ID
    await expect(dialog.locator('td.font-mono', { hasText: 'task-' }).first()).toBeAttached();
  });

  test('View Logs dialog shows numeric columns', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await section.getByTitle('View Logs').first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // Should show Found/Created/Skipped/Duration column headers
    await expect(dialog.getByRole('columnheader', { name: 'Found' })).toBeVisible();
    await expect(dialog.getByRole('columnheader', { name: 'Created' })).toBeVisible();
    await expect(dialog.getByRole('columnheader', { name: 'Skipped' })).toBeVisible();
    await expect(dialog.getByRole('columnheader', { name: 'Duration' })).toBeVisible();
  });

  test('non-admin is redirected away from admin page', async ({ memberPage: page }) => {
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Admin', exact: true })).not.toBeVisible();
    await expect(page).not.toHaveURL(/\/admin/);
  });
});

test.describe.serial('Admin - Intake Sources CRUD', () => {
  const SOURCE_NAME = 'E2E Test Source';
  const EDITED_NAME = 'E2E Test Source Edited';

  test('create an intake source', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await section.getByRole('button', { name: /Add Source/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Add Intake Source')).toBeVisible();

    await dialog.locator('#intake-name').fill(SOURCE_NAME);
    await dialog.locator('#intake-repo').fill('e2eorg/e2erepo');
    await dialog.locator('#intake-branch').fill('develop');
    await dialog.locator('#intake-labels').fill('bug, e2e-test');
    await dialog.locator('#intake-user').fill('e2e-admin');
    await dialog.locator('#intake-interval').fill('60');
    await dialog.locator('#intake-max-concurrent').fill('5');

    await dialog.getByRole('button', { name: 'Create Source' }).click();

    // Wait for the new source to appear in the table
    await expect(section.getByText(SOURCE_NAME)).toBeVisible({ timeout: 10000 });
    await expect(section.getByText('e2eorg/e2erepo')).toBeVisible();
  });

  test('created source persists on fresh load', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await expect(section.getByText(SOURCE_NAME)).toBeVisible();
    await expect(section.getByText('e2eorg/e2erepo')).toBeVisible();
    await expect(section.getByText('60s')).toBeVisible();
    await expect(section.getByText('e2e-admin')).toBeVisible();
  });

  test('edit the intake source', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    const row = section.locator('tr').filter({ hasText: SOURCE_NAME });
    await row.getByTitle('Edit').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Edit Intake Source')).toBeVisible();

    // Verify form is pre-filled with current values
    await expect(dialog.locator('#intake-name')).toHaveValue(SOURCE_NAME);
    await expect(dialog.locator('#intake-repo')).toHaveValue('e2eorg/e2erepo');
    await expect(dialog.locator('#intake-branch')).toHaveValue('develop');
    await expect(dialog.locator('#intake-user')).toHaveValue('e2e-admin');

    // Edit the name and repo
    await dialog.locator('#intake-name').fill(EDITED_NAME);
    await dialog.locator('#intake-repo').fill('e2eorg/edited-repo');
    await dialog.locator('#intake-interval').fill('120');

    await dialog.getByRole('button', { name: 'Save Changes' }).click();

    // Verify updated values in the table
    await expect(section.getByText(EDITED_NAME)).toBeVisible({ timeout: 10000 });
    await expect(section.getByText('e2eorg/edited-repo')).toBeVisible();
  });

  test('edited source persists on fresh load', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await expect(section.getByText(EDITED_NAME)).toBeVisible();
    await expect(section.getByText('e2eorg/edited-repo')).toBeVisible();
    // Original name should be gone
    await expect(section.getByText(SOURCE_NAME, { exact: true })).not.toBeVisible();
  });

  test('delete the intake source', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    const row = section.locator('tr').filter({ hasText: EDITED_NAME });
    await row.getByTitle('Delete').click();

    // Confirm deletion in dialog
    await expect(page.getByText('Delete Intake Source')).toBeVisible();
    await page.getByRole('button', { name: 'Delete', exact: true }).click();

    // Verify it's gone
    await expect(section.getByText(EDITED_NAME)).not.toBeVisible({ timeout: 10000 });
  });

  test('deleted source is gone on fresh load', async ({ adminPage: page }) => {
    await page.goto('/admin');

    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    await expect(section.getByText(EDITED_NAME)).not.toBeVisible();
  });
});
