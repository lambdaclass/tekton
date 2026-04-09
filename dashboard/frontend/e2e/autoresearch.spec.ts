import { test, expect, TEST_IDS } from './fixtures';

test.describe('Autoresearch', () => {
  test('list page shows runs', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('Optimize sort perf')).toBeVisible();
    await expect(adminPage.getByText('Speed up parser')).toBeVisible();
  });

  test('list page shows improvement percentage and experiment counts', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('+20.7%')).toBeVisible();
    await expect(adminPage.getByText('5 experiments (3 accepted)')).toBeVisible();
    await expect(adminPage.getByText('8 experiments (2 accepted)')).toBeVisible();
  });

  test('list page shows status badges', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await expect(adminPage.getByText('completed').first()).toBeVisible();
    await expect(adminPage.getByText('running').first()).toBeVisible();
  });

  test('new run form opens, shows fields, and closes', async ({ adminPage }) => {
    await adminPage.goto('/autoresearch');
    await adminPage.getByRole('button', { name: 'New Run' }).click();
    await expect(adminPage.getByText('New Autoresearch Run')).toBeVisible();
    // Check form fields exist
    await expect(adminPage.getByLabel('Repository')).toBeVisible();
    await expect(adminPage.getByLabel('Base Branch')).toBeVisible();
    await expect(adminPage.getByLabel('Benchmark Command')).toBeVisible();
    await expect(adminPage.getByLabel('Metric Regex (one capture group)')).toBeVisible();
    await expect(adminPage.getByText('Lower is better')).toBeVisible();
    await expect(adminPage.getByText('Higher is better')).toBeVisible();
    await expect(adminPage.getByLabel('Max Experiments')).toBeVisible();
    await expect(adminPage.getByLabel('Target Files')).toBeVisible();
    await expect(adminPage.getByLabel('Frozen Files')).toBeVisible();
    // Close it
    await adminPage.getByRole('button', { name: 'Cancel' }).first().click();
    await expect(adminPage.getByText('New Autoresearch Run')).toHaveCount(0);
  });

  test('detail page shows stats for completed run', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await expect(adminPage.getByText('Optimize sort perf')).toBeVisible();
    await expect(adminPage.getByText('completed')).toBeVisible();
    // Stats bar values
    await expect(adminPage.getByText('42.5000').first()).toBeVisible(); // baseline
    await expect(adminPage.getByText('51.3000').first()).toBeVisible(); // best
    await expect(adminPage.getByText('Improvement', { exact: true })).toBeVisible();
    await expect(adminPage.getByText('Rate', { exact: true })).toBeVisible();
    await expect(adminPage.getByText('Cost', { exact: true }).first()).toBeVisible();
  });

  test('detail page shows experiment feed with accepted and rejected entries', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await expect(adminPage.getByRole('tab', { name: 'Experiments' })).toBeVisible();
    await expect(adminPage.getByText('Switched to a more efficient comparison')).toBeVisible();
    await expect(adminPage.getByText('Attempted parallel sorting')).toBeVisible();
    // Click on an experiment to expand its diff
    await adminPage.locator('button:has-text("Optimized inner loop")').click();
    await expect(adminPage.getByText('diff --git a/src/sort.py').first()).toBeVisible();
  });

  test('detail page shows config tab with run parameters', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await adminPage.getByRole('tab', { name: 'Config' }).click();
    await expect(adminPage.getByText('python benchmark.py')).toBeVisible();
    await expect(adminPage.getByText('Higher is better')).toBeVisible();
    await expect(adminPage.getByText('src/sort.py')).toBeVisible();
    await expect(adminPage.getByText('testorg/testrepo').first()).toBeVisible();
  });

  test('detail page shows logs tab with xterm', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await adminPage.getByRole('tab', { name: 'Logs' }).click();
    await expect(adminPage.locator('.xterm').first()).toBeVisible();
  });

  test('detail page back button navigates to list', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.completed}`);
    await adminPage.getByRole('button', { name: 'Back' }).click();
    await expect(adminPage).toHaveURL('/autoresearch');
  });

  test('running run shows stop button', async ({ adminPage }) => {
    await adminPage.goto(`/autoresearch/${TEST_IDS.autoresearch.running}`);
    await expect(adminPage.getByText('Speed up parser')).toBeVisible();
    await expect(adminPage.getByText('running')).toBeVisible();
  });

  test('navigation includes Autoresearch link', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByRole('link', { name: 'Autoresearch' })).toBeVisible();
  });

  test('member can see autoresearch page', async ({ memberPage }) => {
    await memberPage.goto('/autoresearch');
    await expect(memberPage.getByRole('button', { name: 'New Run' })).toBeVisible();
  });
});

test.describe('Admin: Benchmark Servers', () => {
  test('benchmark servers section is visible in admin panel', async ({ adminPage }) => {
    await adminPage.goto('/admin');
    await expect(adminPage.getByText('Benchmark Servers')).toBeVisible();
  });

  test('add server form opens, fills fields, and closes', async ({ adminPage }) => {
    await adminPage.goto('/admin');
    await adminPage.getByRole('button', { name: 'Add Server' }).click();
    // Fill out the form to exercise state setters
    await adminPage.getByLabel('Name').last().fill('test-gpu');
    await adminPage.getByLabel('Hostname / IP').fill('10.0.0.1');
    await adminPage.getByLabel('SSH User').fill('ubuntu');
    await adminPage.getByLabel('SSH Key Path').fill('/root/.ssh/id_ed25519');
    await adminPage.getByLabel('Hardware Description').fill('4x A100');
    // Cancel instead of submitting
    await adminPage.getByRole('button', { name: 'Cancel' }).last().click();
    // Form should be gone
    await expect(adminPage.getByLabel('Hostname / IP')).toHaveCount(0);
  });
});
