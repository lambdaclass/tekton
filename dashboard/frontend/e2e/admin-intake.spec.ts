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
    await dialog.locator('#intake-token').fill('ghp_test_token_e2e');
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
