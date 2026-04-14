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
    await expect(adminPage.getByText(TEST_IDS.repos.main).first()).toBeVisible();
  });

  test('shows branch name in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('feat/user-settings').first()).toBeVisible();
  });

  test('shows creator in metadata', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText(TEST_IDS.users.admin).first()).toBeVisible();
  });

  test('shows token usage and cost info in metadata bar', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('50,000 in').first()).toBeVisible();
    await expect(adminPage.getByText('20,000 out').first()).toBeVisible();
    await expect(adminPage.getByText('$1.50').first()).toBeVisible();
  });

  test('shows prompt text in left column', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText('Implement user settings page', { exact: true })).toBeVisible();
  });

  test('back button navigates to tasks list', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('button', { name: 'Back to tasks' }).click();
    await expect(adminPage).toHaveURL('/tasks');
  });

  test('shows View PR button for task with PR', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByRole('button', { name: /PR #42/ })).toBeVisible();
  });

  test('shows Agent Logs in Logs tab', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('tab', { name: 'Logs' }).click();
    await expect(adminPage.getByText('Agent Logs')).toBeVisible();
  });

  test('shows Container Logs in Logs tab', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('tab', { name: 'Logs' }).click();
    await expect(adminPage.getByText('Container Logs')).toBeVisible();
  });

  test('shows Diff tab with refresh button', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('tab', { name: 'Diff' }).click();
    await expect(adminPage.getByRole('button', { name: 'Refresh' })).toBeVisible();
  });

  test('shows error message for failed task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.failed}`);
    await adminPage.getByRole('tab', { name: 'Info' }).click();
    await expect(adminPage.getByText('Error').first()).toBeVisible();
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

  test('shows subtasks section in Info tab for parent task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('tab', { name: 'Info' }).click();
    await expect(adminPage.getByText('Subtasks')).toBeVisible();
    await expect(adminPage.getByText('Settings tests')).toBeVisible();
  });

  test('subtask links navigate to subtask detail', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await adminPage.getByRole('tab', { name: 'Info' }).click();
    await expect(adminPage.getByText('Subtasks')).toBeVisible();
    await adminPage.getByText('Settings tests').click();
    await expect(adminPage).toHaveURL(new RegExp(`/tasks/${TEST_IDS.tasks.subtask}`));
  });

  test('subtask detail shows parent task link', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.subtask}`);
    await expect(adminPage.getByText('Parent:').first()).toBeVisible();
    await expect(adminPage.getByText(TEST_IDS.tasks.completed.slice(0, 8))).toBeVisible();
  });

  test('pending task shows prompt text in left column', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.pending}`);
    await expect(adminPage.getByText('Add dark mode support')).toBeVisible();
  });

  test('Live/Disconnected badge is shown for awaiting task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(adminPage.getByText('Disconnected').or(adminPage.getByText('Live'))).toBeVisible();
  });

  test('navigating to non-existent task shows task ID in heading', async ({ adminPage }) => {
    await adminPage.goto('/tasks/nonexist');
    await expect(adminPage.locator('h1')).toContainText('nonexist');
  });

  test('shows policy violation banner', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.getByText(/\d+ policy violation/)).toBeVisible();
    await expect(adminPage.getByText('Bash').first()).toBeVisible();
  });

  test('shows Create PR button for completed task without PR', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completedNoPR}`);
    await expect(adminPage.getByRole('button', { name: 'Create PR' })).toBeVisible();
  });

  test('shows task reference images in Info tab when image_url is set', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.running}`);
    await adminPage.getByRole('tab', { name: 'Info' }).click();
    const images = adminPage.locator('img[alt^="Task reference image"]');
    await expect(images.first()).toBeVisible();
  });

  test('shows Preview tab for task with preview', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(adminPage.getByRole('tab', { name: 'Preview' })).toBeVisible();
  });

  test('preview tab refresh button works', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Preview' }).click();
    const refreshBtn = adminPage.getByRole('button', { name: 'Refresh' });
    await expect(refreshBtn).toBeVisible();
    await refreshBtn.click();
    await expect(refreshBtn).toBeVisible();
  });
});
