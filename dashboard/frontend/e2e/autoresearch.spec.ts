import { test, expect, TEST_IDS } from './fixtures';

test.describe('Autoresearch', () => {
  test('list page shows runs', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('Optimize sort perf')).toBeVisible();
    await expect(adminPage.getByText('Speed up parser')).toBeVisible();
  });

  test('list page shows improvement percentage', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('+20.7%')).toBeVisible();
  });

  test('list page shows experiment counts', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('5 experiments (3 accepted)')).toBeVisible();
  });

  test('new run form opens and closes', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await adminPage.getByRole('button', { name: 'New Run' }).click();
    await expect(adminPage.getByText('New Autoresearch Run')).toBeVisible();
    await adminPage.getByRole('button', { name: 'Cancel' }).first().click();
    await expect(adminPage.getByText('New Autoresearch Run')).toHaveCount(0);
  });

  test('detail page shows stats for completed run', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await expect(adminPage.getByText('Optimize sort perf')).toBeVisible();
    await expect(adminPage.getByText('completed')).toBeVisible();
    await expect(adminPage.getByText('42.5000')).toBeVisible(); // baseline
    await expect(adminPage.getByText('51.3000')).toBeVisible(); // best
  });

  test('detail page shows experiments tab', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await expect(adminPage.getByRole('tab', { name: 'Experiments' })).toBeVisible();
    // Check experiment feed shows entries
    await expect(adminPage.getByText('Switched to a more efficient comparison')).toBeVisible();
  });

  test('detail page shows config tab', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await adminPage.getByRole('tab', { name: 'Config' }).click();
    await expect(adminPage.getByText('python benchmark.py')).toBeVisible();
    await expect(adminPage.getByText('Higher is better')).toBeVisible();
  });

  test('detail page shows logs tab', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await adminPage.getByRole('tab', { name: 'Logs' }).click();
    // LogViewer should render (it's an xterm container)
    await expect(adminPage.locator('.xterm')).toBeVisible();
  });

  test('navigation includes Autoresearch link', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByRole('link', { name: 'Autoresearch' })).toBeVisible();
  });
});
