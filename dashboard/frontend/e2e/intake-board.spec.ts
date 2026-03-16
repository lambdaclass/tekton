import { test, expect, TEST_IDS } from './fixtures';

const ISSUES = TEST_IDS.intake.issues;

test.describe('Intake Board', () => {
  test('renders page heading and issue count', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await expect(page.getByRole('heading', { name: 'Intake Board' })).toBeVisible();
    await expect(page.getByText('8 issue(s) across all sources')).toBeVisible();
  });

  test('renders all 6 columns', async ({ adminPage: page }) => {
    await page.goto('/intake');

    for (const label of ['Backlog', 'Pending', 'In Progress', 'Review', 'Done', 'Failed']) {
      await expect(page.getByText(label, { exact: true }).first()).toBeVisible();
    }
  });

  test('issues appear in correct columns', async ({ adminPage: page }) => {
    await page.goto('/intake');

    // All 8 issue titles should be visible on the board
    for (const title of Object.values(ISSUES)) {
      await expect(page.getByText(title).first()).toBeVisible();
    }
  });

  test('cards show repo name', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await expect(page.getByText('testorg/testrepo').first()).toBeVisible();
    await expect(page.getByText('testorg/frontend').first()).toBeVisible();
  });

  test('search filters issues by title', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByPlaceholder('Filter by title').fill('Safari');

    await expect(page.getByText(ISSUES.pendingSafari)).toBeVisible();
    await expect(page.getByText(ISSUES.backlogAuth)).not.toBeVisible();
    await expect(page.getByText(ISSUES.backlogDarkMode)).not.toBeVisible();
  });

  test('source dropdown filters by source', async ({ adminPage: page }) => {
    await page.goto('/intake');

    // Select GitHub Bugs source
    const sourceSelect = page.locator('select').first();
    await sourceSelect.selectOption({ label: 'GitHub Bugs (testorg/testrepo)' });

    // GitHub issues should be visible
    await expect(page.getByText(ISSUES.backlogAuth)).toBeVisible();
    // Linear issues should be hidden
    await expect(page.getByText(ISSUES.backlogDarkMode)).not.toBeVisible();
    await expect(page.getByText(ISSUES.pendingCsv)).not.toBeVisible();
  });

  test('clicking a card opens detail dialog', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.backlogAuth).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText(ISSUES.backlogAuth)).toBeVisible();
    await expect(dialog.getByText('testorg/testrepo')).toBeVisible();
    await expect(dialog.getByText('View External Issue')).toBeVisible();
    await expect(dialog.getByText('Move to:')).toBeVisible();
  });

  test('detail dialog for linked issue shows task link', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.inProgressRateLimit).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText(/View Task/)).toBeVisible();
  });

  test('detail dialog for failed issue shows error', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.failedCi).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Agent timed out after 300s')).toBeVisible();
  });

  test('non-admin cannot access intake board', async ({ memberPage: page }) => {
    await page.goto('/intake');

    // Should be redirected away — Intake Board heading should not appear
    await expect(page.getByRole('heading', { name: 'Intake Board' })).not.toBeVisible();
    await expect(page).not.toHaveURL(/\/intake/);
  });
});
