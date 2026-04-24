import { test, expect, TEST_IDS } from './fixtures';

test.describe('Activity Sidebar', () => {
  test('activity toggle button is visible in conversation tab', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    const toggleBtn = adminPage.getByRole('button', { name: 'Show activity' });
    await expect(toggleBtn).toBeVisible();
  });

  test('clicking activity toggle opens sidebar with timeline', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('button', { name: 'Show activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toBeVisible();
    // Verify timeline renders seeded actions (clone, commit, push are "important" single actions)
    await expect(adminPage.getByText('Cloned testorg/testrepo')).toBeVisible();
    await expect(adminPage.getByText('Created commit: fix button alignment')).toBeVisible();
    await expect(adminPage.getByText('Pushed to branch fix/button-align')).toBeVisible();
  });

  test('grouped actions can be expanded', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('button', { name: 'Show activity' }).click();
    // The two consecutive Read tool_use actions are grouped as "Read × 2"
    const groupBtn = adminPage.getByText('Read × 2');
    await expect(groupBtn).toBeVisible();
    // Click to expand the group
    await groupBtn.click();
    // Expanded content shows individual action summaries (paths shortened by shortenPath)
    await expect(adminPage.getByText(/Reading.*components\/Button\.tsx/)).toBeVisible();
    await expect(adminPage.getByText(/Reading.*styles\/layout\.css/)).toBeVisible();
  });

  test('clicking activity toggle again closes sidebar', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('button', { name: 'Show activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toBeVisible();
    await adminPage.getByRole('button', { name: 'Hide activity' }).click();
    await expect(adminPage.locator('h3').filter({ hasText: 'Activity' })).toHaveCount(0);
  });
});
