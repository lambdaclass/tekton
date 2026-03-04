import { test, expect, TEST_IDS } from './fixtures';

test.describe('Activity Sidebar', () => {
  test('activity toggle button is visible in conversation tab', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    // Conversation tab is default for awaiting_followup tasks
    const toggleBtn = adminPage.getByTitle('Show activity');
    await expect(toggleBtn).toBeVisible();
  });

  test('clicking activity toggle opens sidebar with timeline', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByTitle('Show activity').click();
    await expect(adminPage.getByText('Activity')).toBeVisible();
  });

  test('clicking activity toggle again closes sidebar', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByTitle('Show activity').click();
    await expect(adminPage.getByText('Activity')).toBeVisible();
    await adminPage.getByTitle('Hide activity').click();
    // The "Activity" heading inside the sidebar should be gone
    await expect(adminPage.locator('h3:has-text("Activity")')).toHaveCount(0);
  });
});
