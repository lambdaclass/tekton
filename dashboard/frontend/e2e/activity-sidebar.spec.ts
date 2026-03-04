import { test, expect, TEST_IDS } from './fixtures';

test.describe('Activity Sidebar', () => {
  test('activity toggle button is visible in conversation tab', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    // Click Conversation tab (activity toggle is inside it)
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    const toggleBtn = adminPage.getByRole('button', { name: 'Show activity' });
    await expect(toggleBtn).toBeVisible();
  });

  test('clicking activity toggle opens sidebar with timeline', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    await adminPage.getByRole('button', { name: 'Show activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toBeVisible();
  });

  test('clicking activity toggle again closes sidebar', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    await adminPage.getByRole('button', { name: 'Show activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toBeVisible();
    await adminPage.getByRole('button', { name: 'Hide activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toHaveCount(0);
  });
});
