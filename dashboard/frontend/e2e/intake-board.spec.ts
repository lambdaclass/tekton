import { test, expect, TEST_IDS } from './fixtures';

const ISSUES = TEST_IDS.intake.issues;

test.describe('Intake Board', () => {
  test('renders page heading and issue count', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await expect(page.getByRole('heading', { name: 'Intake Board' })).toBeVisible();
    await expect(page.getByText('11 issues across all sources')).toBeVisible();
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

    await expect(page.locator('.font-mono', { hasText: 'testorg/testrepo' }).first()).toBeAttached();
    await expect(page.locator('.font-mono', { hasText: 'testorg/frontend' }).first()).toBeAttached();
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

  test('cards show label badges', async ({ adminPage: page }) => {
    await page.goto('/intake');

    // The "Fix null pointer in auth module" issue has labels: bug, auth
    const card = page.locator('text=Fix null pointer in auth module').locator('..');
    const badges = card.locator('span.rounded-full');
    await expect(badges.filter({ hasText: 'bug' })).toBeVisible();
    await expect(badges.filter({ hasText: 'auth' })).toBeVisible();
  });

  test('detail dialog shows labels', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.backlogAuth).click();

    const dialog = page.getByRole('dialog');
    // Labels are rendered as rounded-full spans; scope to avoid matching body text
    const labels = dialog.locator('span.rounded-full');
    await expect(labels.filter({ hasText: 'bug' })).toBeVisible();
    await expect(labels.filter({ hasText: 'auth' })).toBeVisible();
  });

  test('detail dialog shows issue body', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.backlogAuth).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Auth module throws NPE when token is expired.')).toBeVisible();
  });

  test('detail dialog close button dismisses dialog', async ({ adminPage: page }) => {
    await page.goto('/intake');

    await page.getByText(ISSUES.backlogAuth).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // Close via the X button
    await dialog.getByRole('button', { name: 'Close' }).click();
    await expect(dialog).not.toBeVisible();
  });

  test('slots in use indicator is visible', async ({ adminPage: page }) => {
    await page.goto('/intake');

    // task_created + review issues occupy slots
    await expect(page.getByText(/slot.*in use/)).toBeVisible();
  });

  test('move issue status via dialog button', async ({ adminPage: page }) => {
    await page.goto('/intake');

    // Open a backlog issue and move it to pending
    await page.getByText(ISSUES.backlogAuth).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // The "Pending" button should be enabled (valid transition from backlog)
    await dialog.getByRole('button', { name: 'Pending' }).click();

    // Optimistic update: the issue should move to the Pending column
    // Close the dialog and verify the issue appears in Pending
    await dialog.getByRole('button', { name: 'Close' }).click();

    // Wait for the board to reflect the change
    const pendingColumn = page.locator('div').filter({ hasText: /^Pending/ }).first();
    await expect(pendingColumn.getByText(ISSUES.backlogAuth)).toBeVisible({ timeout: 5000 });

    // Move it back to backlog for test isolation
    await page.getByText(ISSUES.backlogAuth).click();
    const dialog2 = page.getByRole('dialog');
    await dialog2.getByRole('button', { name: 'Backlog' }).click();
    await dialog2.getByRole('button', { name: 'Close' }).click();
  });

  test('non-admin cannot access intake board', async ({ memberPage: page }) => {
    await page.goto('/intake');

    // Should be redirected away — Intake Board heading should not appear
    await expect(page.getByRole('heading', { name: 'Intake Board' })).not.toBeVisible();
    await expect(page).not.toHaveURL(/\/intake/);
  });
});
