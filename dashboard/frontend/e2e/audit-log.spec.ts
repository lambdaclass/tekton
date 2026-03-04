import { test, expect, TEST_IDS } from './fixtures';

test.describe('Audit Log', () => {
  test('renders Audit Log heading', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible();
  });

  test('renders filter controls', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await expect(page.getByText('Event Type')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by actor...')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by target...')).toBeVisible();
    await expect(page.getByText('Start Date')).toBeVisible();
    await expect(page.getByText('End Date')).toBeVisible();
  });

  test('renders audit log entries in table', async ({ adminPage: page }) => {
    await page.goto('/audit');

    // Table headers
    await expect(page.getByRole('columnheader', { name: 'Timestamp' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Event Type' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Actor' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Target' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Detail' })).toBeVisible();

    // Seeded entry data
    await expect(page.getByText('auth.login').first()).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testadmin' }).first()).toBeVisible();
  });

  test('total count is displayed', async ({ adminPage: page }) => {
    await page.goto('/audit');

    // Flexible count — don't assert exact number since other tests may add entries
    await expect(page.getByText(/\(\d+ total\)/)).toBeVisible();
  });

  test('detail expand/collapse toggle works', async ({ adminPage: page }) => {
    await page.goto('/audit');

    // Click the first "Show" button to expand details
    const showButton = page.getByRole('button', { name: 'Show' }).first();
    await showButton.click();

    // The detail JSON should be visible in a pre block
    await expect(page.locator('pre').first()).toBeVisible();

    // Now it should say "Hide"
    const hideButton = page.getByRole('button', { name: 'Hide' }).first();
    await hideButton.click();

    // Pre block should not be visible
    await expect(page.locator('pre')).not.toBeVisible();
  });

  test('task target is a clickable link', async ({ adminPage: page }) => {
    await page.goto('/audit');

    const targetLink = page.getByRole('link', { name: 'task-completed-1' }).first();
    await expect(targetLink).toBeVisible();
    await expect(targetLink).toHaveAttribute('href', '/tasks/task-completed-1');
  });

  test('pagination is shown with 27+ entries', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await expect(page.getByText(/Page 1/)).toBeVisible();
    await expect(page.getByRole('button', { name: /Prev/ })).toBeDisabled();
    await expect(page.getByRole('button', { name: /Next/ })).toBeEnabled();
  });

  test('clicking Next navigates to page 2', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await page.getByRole('button', { name: /Next/ }).click();

    await expect(page.getByText(/Page 2/)).toBeVisible();
  });

  test('actor filter filters results', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await page.getByPlaceholder('Filter by actor...').fill('testadmin');

    // Wait for filter to take effect — count should update
    await expect(page.getByRole('cell', { name: 'testadmin' }).first()).toBeVisible();
    // After filtering by actor=testadmin, the "system" actor entries should be gone
    await expect(page.getByRole('cell', { name: 'system' })).not.toBeVisible({ timeout: 5000 });
  });

  test('event type filter narrows results', async ({ adminPage: page }) => {
    await page.goto('/audit');

    // Open the shadcn Select for event type
    await page.getByRole('combobox').click();

    // Select admin.user_repos_changed — exists in both dropdown and seed data
    await page.getByRole('option', { name: 'admin.user_repos_changed' }).click();

    // Verify filter applied — matching entries visible
    await expect(page.getByText('admin.user_repos_changed').first()).toBeVisible();
    // auth.login entries should be gone
    await expect(page.getByText('auth.login')).not.toBeVisible({ timeout: 5000 });
  });

  test('target filter filters results', async ({ adminPage: page }) => {
    await page.goto('/audit');

    await page.getByPlaceholder('Filter by target...').fill('task-completed-1');

    // Should show entries targeting task-completed-1
    await expect(page.getByRole('link', { name: 'task-completed-1' }).first()).toBeVisible();
    // Entries for other targets should be gone
    await expect(page.getByText('task-pending-1')).not.toBeVisible({ timeout: 5000 });
  });

  test('combining filters narrows results further', async ({ adminPage: page }) => {
    await page.goto('/audit');

    // Filter by actor=testadmin AND target=task-completed-1
    await page.getByPlaceholder('Filter by actor...').fill('testadmin');
    await page.getByPlaceholder('Filter by target...').fill('task-completed-1');

    // Should only show testadmin entries targeting task-completed-1
    await expect(page.getByRole('cell', { name: 'testadmin' }).first()).toBeVisible();
    await expect(page.getByRole('link', { name: 'task-completed-1' }).first()).toBeVisible();
    // Should NOT show entries from other actors
    await expect(page.getByRole('cell', { name: 'system' })).not.toBeVisible({ timeout: 5000 });
  });

  test('non-admin user is redirected away from audit log', async ({ memberPage: page }) => {
    await page.goto('/audit');
    await expect(page).toHaveURL('/');
  });
});
