import { test, expect, TEST_IDS } from './fixtures';

test.describe('Authentication', () => {
  test('authenticated admin user sees their username in sidebar', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByText(TEST_IDS.users.admin)).toBeVisible();
  });

  test('authenticated member user sees their username in sidebar', async ({ memberPage }) => {
    await memberPage.goto('/');
    await expect(memberPage.getByText(TEST_IDS.users.member)).toBeVisible();
  });

  test('authenticated user sees role badge', async ({ adminPage }) => {
    await adminPage.goto('/');
    // The role badge in the sidebar footer
    const roleBadge = adminPage.locator('[data-sidebar="footer"]').getByText('admin', { exact: true });
    await expect(roleBadge).toBeVisible();
  });

  test('viewer role badge is displayed for viewer user', async ({ viewerPage }) => {
    await viewerPage.goto('/');
    const roleBadge = viewerPage.locator('[data-sidebar="footer"]').getByText('viewer', { exact: true });
    await expect(roleBadge).toBeVisible();
  });

  test('admin user sees admin nav items', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByRole('link', { name: 'Admin' })).toBeVisible();
    await expect(adminPage.getByRole('link', { name: 'Cost' })).toBeVisible();
    await expect(adminPage.getByRole('link', { name: 'Audit Log' })).toBeVisible();
  });

  test('member user does not see admin nav items', async ({ memberPage }) => {
    await memberPage.goto('/');
    await expect(memberPage.getByRole('link', { name: 'Admin' })).toHaveCount(0);
    await expect(memberPage.getByRole('link', { name: 'Cost' })).toHaveCount(0);
    await expect(memberPage.getByRole('link', { name: 'Audit Log' })).toHaveCount(0);
  });

  test('viewer user does not see admin nav items', async ({ viewerPage }) => {
    await viewerPage.goto('/');
    await expect(viewerPage.getByRole('link', { name: 'Admin' })).toHaveCount(0);
  });

  test('logout button is visible', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByText('Logout')).toBeVisible();
  });

  test('viewer cannot see New Task button', async ({ viewerPage }) => {
    await viewerPage.goto('/tasks');
    await expect(viewerPage.getByRole('heading', { name: 'Tasks' })).toBeVisible();
    await expect(viewerPage.getByRole('button', { name: 'New Task' })).toHaveCount(0);
  });

  test('member can see New Task button', async ({ memberPage }) => {
    await memberPage.goto('/tasks');
    await expect(memberPage.getByRole('button', { name: 'New Task' })).toBeVisible();
  });
});

test.describe('Logout', () => {
  test('clicking logout redirects to sign-in page', async ({ adminPage }) => {
    await adminPage.goto('/');
    await expect(adminPage.getByText('testadmin')).toBeVisible();

    // Click the logout button
    await adminPage.getByText('Logout').click();

    // Should redirect to sign-in page
    await expect(adminPage.getByText('Sign in with your GitHub account')).toBeVisible({ timeout: 10000 });
    await expect(adminPage.getByRole('link', { name: 'Sign in with GitHub' })).toBeVisible();
  });
});

test.describe('Unauthenticated', () => {
  test('unauthenticated user sees sign-in page', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByText('Sign in with your GitHub account')).toBeVisible();
    await expect(page.getByText('Sign in with GitHub')).toBeVisible();
  });

  test('sign in link points to GitHub OAuth', async ({ page }) => {
    await page.goto('/');
    const signInLink = page.getByRole('link', { name: 'Sign in with GitHub' });
    await expect(signInLink).toHaveAttribute('href', '/api/auth/login');
  });

  test('unauthenticated user sees Preview Dashboard title', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Preview Dashboard' })).toBeVisible();
  });
});
