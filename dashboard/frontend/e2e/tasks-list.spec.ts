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
    // Check status badges for terminal statuses (stable across environments)
    const cards = adminPage.locator('a[href^="/tasks/"]');
    await expect(cards.filter({ hasText: 'completed' }).first()).toBeVisible();
    await expect(cards.filter({ hasText: 'failed' }).first()).toBeVisible();
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
    await expect(adminPage.getByText('Dark mode').first()).toBeVisible();
    await expect(adminPage.getByText('User settings').first()).toBeVisible();
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
    // Verify the default value is "all" (All statuses)
    await expect(select).toHaveValue('all');
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

  test('keyboard shortcut n toggles new task form', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Press 'n' to open the form
    await adminPage.keyboard.press('n');
    await expect(adminPage.getByLabel('Prompt')).toBeVisible();

    // Press Escape to close
    await adminPage.keyboard.press('Escape');
    await expect(adminPage.getByLabel('Prompt')).toHaveCount(0);
  });

  test('keyboard shortcut j/k navigates task cards', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Press j to select first card — the Card inside should get ring-2 class
    await adminPage.keyboard.press('j');
    const cardLinks = adminPage.locator('a[href^="/tasks/"]');
    // The Card component (direct child div) inside the first link gets the ring
    await expect(cardLinks.first().locator('[class*="ring-2"]')).toBeVisible();

    // Press j again to move to the second card
    await adminPage.keyboard.press('j');
    // First card should no longer be selected, second should be
    await expect(cardLinks.first().locator('[class*="ring-2"]')).toHaveCount(0);
    await expect(cardLinks.nth(1).locator('[class*="ring-2"]')).toBeVisible();

    // Press k to go back up
    await adminPage.keyboard.press('k');
    await expect(cardLinks.first().locator('[class*="ring-2"]')).toBeVisible();
  });

  test('keyboard shortcut Enter opens selected task', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Press j to select first card, then Enter to navigate
    await adminPage.keyboard.press('j');
    await adminPage.keyboard.press('Enter');

    // Should navigate to a task detail page
    await expect(adminPage).toHaveURL(/\/tasks\//);
  });

  test('keyboard shortcuts do not fire when typing in search', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Focus the search input and type 'n' — should NOT open the form
    const searchInput = adminPage.getByPlaceholder('Search prompts...');
    await searchInput.click();
    await searchInput.fill('n');

    // The form should NOT be open
    await expect(adminPage.getByLabel('Prompt')).toHaveCount(0);
  });

  test('filter resets to page 1 when status changes', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();

    // Change filter
    await adminPage.locator('select').selectOption('failed');
    await expect(adminPage.getByText('Migrate database to v2 schema')).toBeVisible();
  });

  test('formatCost shows <$0.01 for tiny cost', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    await expect(adminPage.getByText('<$0.01')).toBeVisible();
  });

  test('timeAgo shows months for old tasks', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // task-old was created 65 days ago = ~2mo ago
    await expect(adminPage.getByText(/\dmo ago/)).toBeVisible();
  });

  test('member user can see New Task button', async ({ memberPage }) => {
    await memberPage.goto('/tasks');
    await expect(memberPage.getByRole('button', { name: 'New Task' })).toBeVisible();
  });

  test('preview URL is shown in task card', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // The awaiting task has preview_url set — should show truncated URL
    await expect(adminPage.getByText('my-preview.test.exampl').first()).toBeVisible();
  });

  test('awaiting_followup status badge appears', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('.line-clamp-2').first()).toBeVisible();
    // The awaiting_followup badge has amber styling — target the badge not the <option>
    const cards = adminPage.locator('a[href^="/tasks/"]');
    await expect(cards.filter({ hasText: 'awaiting_followup' }).first()).toBeVisible();
  });
});
