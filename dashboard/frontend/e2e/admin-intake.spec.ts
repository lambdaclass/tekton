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
