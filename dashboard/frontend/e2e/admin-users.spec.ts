import { test, expect, TEST_IDS } from './fixtures';

test.describe('Admin - Users section', () => {
  test('renders Admin heading and Users section', async ({ adminPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
    );
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [
          { login: 'testadmin', name: 'Test Admin', role: 'admin' },
          { login: 'testmember', name: 'Test Member', role: 'member' },
          { login: 'testviewer', name: 'Test Viewer', role: 'viewer' },
        ],
      })
    );
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Admin', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Users' })).toBeVisible();
  });

  test('user list shows login, name, and role columns', async ({ adminPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
    );
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [
          { login: 'testadmin', name: 'Test Admin', role: 'admin' },
          { login: 'testmember', name: 'Test Member', role: 'member' },
          { login: 'testviewer', name: 'Test Viewer', role: 'viewer' },
        ],
      })
    );
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    // Table headers
    await expect(page.getByRole('columnheader', { name: 'Login' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Role' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Repos' })).toBeVisible();

    // User data
    await expect(page.getByRole('cell', { name: 'testadmin' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'Test Admin' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testmember' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'Test Member' })).toBeVisible();
  });

  test('role select dropdown has admin, member, viewer options', async ({ adminPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
    );
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [{ login: 'testmember', name: 'Test Member', role: 'member' }],
      })
    );
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    const select = page.locator('select').first();
    await expect(select).toBeVisible();

    const options = select.locator('option');
    await expect(options).toHaveCount(3);
    await expect(options.nth(0)).toHaveText('admin');
    await expect(options.nth(1)).toHaveText('member');
    await expect(options.nth(2)).toHaveText('viewer');
  });

  test('changing role calls the API', async ({ adminPage: page }) => {
    let rolePutBody: unknown;
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
    );
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [{ login: 'testmember', name: 'Test Member', role: 'member' }],
      })
    );
    await page.route('**/api/admin/users/testmember/role', async (route) => {
      rolePutBody = JSON.parse(route.request().postData() ?? '{}');
      await route.fulfill({ json: { login: 'testmember', role: 'viewer' } });
    });
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    const select = page.locator('select').first();
    await select.selectOption('viewer');

    await expect(() => {
      expect(rolePutBody).toEqual({ role: 'viewer' });
    }).toPass({ timeout: 5000 });
  });

  test('Manage button opens repo permissions dialog', async ({ adminPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
    );
    await page.route('**/api/admin/users', (route) =>
      route.fulfill({
        json: [{ login: 'testmember', name: 'Test Member', role: 'member' }],
      })
    );
    await page.route('**/api/admin/users/testmember/repos', (route) =>
      route.fulfill({ json: ['testorg/testrepo', 'testorg/frontend'] })
    );
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await page.getByRole('button', { name: /Manage/ }).click();

    await expect(page.getByText('Repo permissions for testmember')).toBeVisible();
    await expect(page.getByText('testorg/testrepo')).toBeVisible();
    await expect(page.getByText('testorg/frontend')).toBeVisible();
  });

  test('non-admin user is redirected away from admin page', async ({ memberPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.member, name: 'Test Member', role: 'member' } })
    );
    await page.goto('/admin');

    // Should redirect to home
    await expect(page).toHaveURL('/');
  });
});
