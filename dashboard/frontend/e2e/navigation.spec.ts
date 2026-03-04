import { test, expect, TEST_IDS } from './fixtures';

test.describe('Navigation', () => {
  test('home page renders with dashboard heading', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.locator('h1')).toHaveText('Preview Dashboard');
  });

  test('tasks page renders with heading', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await expect(adminPage.locator('h1')).toHaveText('Tasks');
  });

  test('task detail page renders for a valid task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.completed}`);
    await expect(adminPage.locator('h1')).toContainText('User settings');
  });

  test('previews page renders', async ({ adminPage }) => {
    await adminPage.goto('/previews');
    await expect(adminPage).toHaveURL('/previews');
  });

  test('admin page renders for admin user', async ({ adminPage }) => {
    await adminPage.goto('/admin');
    await expect(adminPage).toHaveURL('/admin');
  });

  test('cost page renders for admin user', async ({ adminPage }) => {
    await adminPage.goto('/cost');
    await expect(adminPage).toHaveURL('/cost');
  });

  test('audit page renders for admin user', async ({ adminPage }) => {
    await adminPage.goto('/audit');
    await expect(adminPage).toHaveURL('/audit');
  });

  test('settings page renders', async ({ adminPage }) => {
    await adminPage.goto('/settings');
    await expect(adminPage).toHaveURL('/settings');
  });

  test('sidebar nav links navigate to correct pages', async ({ adminPage }) => {
    await adminPage.goto('/');
    const sidebar = adminPage.locator('[data-sidebar="sidebar"]').first();

    // Click Tasks in sidebar
    await sidebar.getByRole('link', { name: 'Tasks' }).click();
    await expect(adminPage).toHaveURL('/tasks');

    // Click Previews in sidebar
    await sidebar.getByRole('link', { name: 'Previews' }).click();
    await expect(adminPage).toHaveURL('/previews');

    // Click Settings in sidebar
    await sidebar.getByRole('link', { name: 'Settings' }).click();
    await expect(adminPage).toHaveURL('/settings');

    // Click Home in sidebar
    await sidebar.getByRole('link', { name: 'Home' }).click();
    await expect(adminPage).toHaveURL('/');
  });

  test('admin sidebar items are visible for admin user', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByRole('link', { name: 'Admin' })).toBeVisible();
    await expect(adminPage.getByRole('link', { name: 'Cost' })).toBeVisible();
    await expect(adminPage.getByRole('link', { name: 'Audit Log' })).toBeVisible();
  });

  test('admin sidebar items are not visible for member user', async ({ memberPage }) => {
    await memberPage.goto('/');
    await expect(memberPage.getByRole('link', { name: 'Admin' })).toHaveCount(0);
    await expect(memberPage.getByRole('link', { name: 'Cost' })).toHaveCount(0);
    await expect(memberPage.getByRole('link', { name: 'Audit Log' })).toHaveCount(0);
  });

  test('sidebar shows active state for current route', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    // The active nav item gets data-active attribute from SidebarMenuButton
    const tasksLink = adminPage.locator('[data-active="true"]').filter({ hasText: 'Tasks' });
    await expect(tasksLink).toBeVisible();
  });

  test('header breadcrumb shows current page name', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    const header = adminPage.locator('header');
    await expect(header).toContainText('Tasks');
  });

  test('sidebar footer shows logged in username', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByText(TEST_IDS.users.admin)).toBeVisible();
  });

  test('sidebar footer shows user role badge', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.locator('[data-sidebar="footer"]').getByText('admin', { exact: true })).toBeVisible();
  });

  test('home page shows Previews and Tasks cards', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByText('Previews').first()).toBeVisible();
    await expect(adminPage.getByText('Tasks').first()).toBeVisible();
    await expect(adminPage.getByText('Manage preview containers')).toBeVisible();
    await expect(adminPage.getByText('Submit coding tasks to your AI agent')).toBeVisible();
  });

  test('home page cards link to correct pages', async ({ adminPage }) => {
    await adminPage.goto('/');

    // Click Previews card
    await adminPage.locator('a[href="/previews"]').first().click();
    await expect(adminPage).toHaveURL('/previews');

    await adminPage.goto('/');

    // Click Tasks card
    await adminPage.locator('a[href="/tasks"]').first().click();
    await expect(adminPage).toHaveURL('/tasks');
  });

  test('layout has sidebar and main content area', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.locator('[data-sidebar="sidebar"]').first()).toBeVisible();
    await expect(adminPage.locator('main').first()).toBeVisible();
  });
});
