import { test, expect, TEST_IDS } from './fixtures';

test.describe('Tasks List', () => {
  test('renders task list with seeded tasks', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.getByRole('heading', { name: 'Tasks' })).toBeVisible();
    // Wait for tasks to load (skeleton disappears)
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
  });

  test('each task card shows status badge', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // Check various status badges from seed data
    await expect(adminPage.getByText('pending').first()).toBeVisible();
    await expect(adminPage.getByText('completed').first()).toBeVisible();
    await expect(adminPage.getByText('failed')).toBeVisible();
  });

  test('task card shows prompt text', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.getByText('Add dark mode support')).toBeVisible();
    await expect(adminPage.getByText('Implement user settings page')).toBeVisible();
  });

  test('task card shows repo name', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.getByText(TEST_IDS.repos.main).first()).toBeVisible();
  });

  test('task card shows task name when available', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.getByText('Dark mode')).toBeVisible();
    await expect(adminPage.getByText('User settings')).toBeVisible();
  });

  test('task card shows truncated task ID', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    // Seed task IDs start with "task-pen", "task-run", etc.
    await expect(adminPage.getByText('task-pen')).toBeVisible();
  });

  test('status filter dropdown works', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Filter by completed status
    await adminPage.locator('select').selectOption('completed');
    // Should show completed tasks and not pending ones
    await expect(adminPage.getByText('Implement user settings page')).toBeVisible();
    await expect(adminPage.getByText('Add dark mode support')).not.toBeVisible();
  });

  test('status filter shows all statuses option', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    const select = adminPage.locator('select');
    await expect(select).toBeVisible();
    await expect(select.locator('option', { hasText: 'All statuses' })).toBeVisible();
  });

  test('search input filters tasks by text', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Search for a specific task
    await adminPage.getByPlaceholder('Search prompts...').fill('dark mode');
    // Wait for debounce
    await adminPage.waitForTimeout(500);

    await expect(adminPage.getByText('Add dark mode support')).toBeVisible();
  });

  test('empty state renders when no results match filter', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Search for something that won't match
    await adminPage.getByPlaceholder('Search prompts...').fill('xyznonexistent12345');
    await adminPage.waitForTimeout(500);

    await expect(adminPage.getByText('No matching tasks')).toBeVisible();
    await expect(adminPage.getByText('Try adjusting your search or filters')).toBeVisible();
  });

  test('New Task button is visible for non-viewer users', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.getByRole('button', { name: 'New Task' })).toBeVisible();
  });

  test('clicking a task card navigates to task detail', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Click the completed task card
    await adminPage.getByText('Implement user settings page').click();
    await expect(adminPage).toHaveURL(new RegExp(`/tasks/${TEST_IDS.tasks.completed}`));
  });

  test('task card shows cost when available', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // The completed task has total_cost_usd = 1.50, should display as $1.50
    await expect(adminPage.getByText('$1.50')).toBeVisible();
  });

  test('task card shows PR link when available', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // task-completed-1 has pr_number = 42
    await expect(adminPage.getByText('PR #42')).toBeVisible();
  });

  test('filter resets to page 1 when status changes', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Change filter
    await adminPage.locator('select').selectOption('failed');
    await expect(adminPage.getByText('Migrate database to v2 schema')).toBeVisible();
  });
});
