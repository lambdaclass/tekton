import { test, expect } from './fixtures';

test.describe('Command Palette', () => {
  test('opens with Ctrl+K shortcut', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.keyboard.press('Control+k');
    // The command palette search input should appear
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toBeVisible();
  });

  test('closes with Escape', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.keyboard.press('Control+k');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toBeVisible();
    await adminPage.keyboard.press('Escape');
    await expect(adminPage.getByPlaceholder('Search tasks, pages...')).toHaveCount(0);
  });

  test('shows navigation items', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.keyboard.press('Control+k');
    await expect(adminPage.getByText('Home')).toBeVisible();
    await expect(adminPage.getByText('Tasks')).toBeVisible();
  });

  test('navigates to selected page', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.keyboard.press('Control+k');
    // Type to filter, then select Tasks
    await adminPage.getByPlaceholder('Search tasks, pages...').fill('Tasks');
    await adminPage.keyboard.press('Enter');
    await expect(adminPage).toHaveURL('/tasks');
  });
});
