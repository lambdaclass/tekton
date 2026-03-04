import { test, expect } from './fixtures';

test.describe('Command Palette', () => {
  test('opens with Ctrl+K shortcut', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toBeVisible({ timeout: 5000 });
  });

  test('closes with Escape', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toBeVisible({ timeout: 5000 });
    await adminPage.keyboard.press('Escape');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toHaveCount(0, { timeout: 5000 });
  });

  test('shows navigation items', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toBeVisible({ timeout: 5000 });
    // Pages group should contain navigation items
    await expect(adminPage.locator('[cmdk-item]').filter({ hasText: 'Tasks' })).toBeVisible();
  });

  test('navigates to selected page', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    const input = adminPage.getByPlaceholder('Search tasks, pages...');
    await expect(input).toBeVisible({ timeout: 5000 });
    await input.fill('Previews');
    // Select the item
    await adminPage.locator('[cmdk-item]').filter({ hasText: 'Previews' }).click();
    await expect(adminPage).toHaveURL('/previews');
  });

  test('shows tasks in palette and navigates to one', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    const input = adminPage.getByPlaceholder('Search tasks, pages...');
    await expect(input).toBeVisible({ timeout: 5000 });
    // Search for a seeded task
    await input.fill('user settings');
    const taskItem = adminPage.locator('[cmdk-item]').filter({ hasText: 'User settings' });
    await expect(taskItem).toBeVisible();
    await taskItem.click();
    await expect(adminPage).toHaveURL(/\/tasks\/task-completed-1/);
  });

  test('Create new task action navigates to tasks page', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.waitForLoadState('networkidle');
    await adminPage.keyboard.press('Control+k');
    const input = adminPage.getByPlaceholder('Search tasks, pages...');
    await expect(input).toBeVisible({ timeout: 5000 });
    await input.fill('Create new');
    await adminPage.locator('[cmdk-item]').filter({ hasText: 'Create new task' }).click();
    await expect(adminPage).toHaveURL(/\/tasks/);
  });
});
