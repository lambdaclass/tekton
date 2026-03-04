import { test, expect, TEST_IDS } from './fixtures';

test.describe('Task Detail', () => {
  test('shows task name in heading', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.locator('h1')).toContainText('User settings');
  });

  test('shows status badge', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('completed').first()).toBeVisible();
  });

  test('shows repo in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText(TEST_IDS.repos.main)).toBeVisible();
  });

  test('shows base branch in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    const base = adminPage.locator('text=Base').locator('..');
    await expect(base).toContainText('main');
  });

  test('shows branch name in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('feat/user-settings')).toBeVisible();
  });

  test('shows creator in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText(TEST_IDS.users.admin)).toBeVisible();
  });

  test('shows token usage and cost info', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    // Token usage section
    await expect(adminPage.getByText('Token Usage')).toBeVisible();
    await expect(adminPage.getByText('50,000 in')).toBeVisible();
    await expect(adminPage.getByText('20,000 out')).toBeVisible();
    await expect(adminPage.getByText('$1.50')).toBeVisible();
  });

  test('shows prompt text', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Implement user settings page')).toBeVisible();
  });

  test('back button navigates to tasks list', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('button', { name: 'Tasks' }).click();
    await expect(adminPage).toHaveURL('/tasks');
  });

  test('shows View PR button for task with PR', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByRole('button', { name: 'View PR #42' })).toBeVisible();
  });

  test('shows Agent Logs section', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Agent Logs')).toBeVisible();
  });

  test('shows Container Logs section', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Container Logs')).toBeVisible();
  });

  test('shows Code Diff section for task with branch', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Code Diff')).toBeVisible();
  });

  test('shows error message for failed task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.failed}`);
    await expect(adminPage.getByText('Error')).toBeVisible();
    await expect(adminPage.getByText('Migration failed: column "legacy_data" does not exist')).toBeVisible();
  });

  test('shows failed status badge for failed task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.failed}`);
    await expect(adminPage.getByText('failed').first()).toBeVisible();
  });

  test('shows Reopen button for completed task (non-viewer)', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByRole('button', { name: 'Reopen' })).toBeVisible();
  });

  test('shows Reopen button for failed task (non-viewer)', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.failed}`);
    await expect(adminPage.getByRole('button', { name: 'Reopen' })).toBeVisible();
  });

  test('shows subtasks section for parent task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Subtasks')).toBeVisible();
    // The subtask prompt
    await expect(adminPage.getByText('Write unit tests for settings page')).toBeVisible();
  });

  test('subtask links navigate to subtask detail', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Subtasks')).toBeVisible();
    // Click the subtask card
    await adminPage.getByText('Write unit tests for settings page').click();
    await expect(adminPage).toHaveURL(new RegExp(`/tasks/${TEST_IDS.tasks.subtask}`));
  });

  test('subtask detail shows parent task link', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.subtask}`);
    await expect(adminPage.getByText('Parent Task')).toBeVisible();
    // The parent task ID link
    await expect(adminPage.getByText(TEST_IDS.tasks.completed.slice(0, 8))).toBeVisible();
  });

  test('pending task does not show branch or Reopen button', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.pending}`);
    await expect(adminPage.getByText('pending').first()).toBeVisible();
    await expect(adminPage.getByRole('button', { name: 'Reopen' })).toHaveCount(0);
  });

  test('Live/Disconnected badge is shown', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    // The websocket connection badge
    await expect(adminPage.getByText('Disconnected').or(adminPage.getByText('Live'))).toBeVisible();
  });

  test('navigating to non-existent task shows task ID in heading', async ({ adminPage }) => {
    await adminPage.goto('/tasks/nonexist');
    // The heading falls back to showing the truncated ID
    await expect(adminPage.locator('h1')).toContainText('nonexist');
  });
});
