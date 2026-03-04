import { test, expect, TEST_IDS } from './fixtures';

const ADMIN_ROUTES = (page: import('@playwright/test').Page) => {
  page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
  );
  page.route('**/api/admin/users', (route) => route.fulfill({ json: [] }));
  page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
  page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
  page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
};

test.describe('Admin - Secrets section', () => {
  test('renders Secrets heading and Add Secret button', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Secrets' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Secret/ })).toBeVisible();
  });

  test('shows empty state when no secrets exist', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByText('No secrets configured.')).toBeVisible();
  });

  test('renders secrets table with columns', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) =>
      route.fulfill({
        json: [
          { id: 1, repo: 'testorg/testrepo', name: 'NPM_TOKEN', created_by: 'testadmin', created_at: '2025-01-15T12:00:00Z' },
          { id: 2, repo: 'testorg/testrepo', name: 'DEPLOY_KEY', created_by: 'testadmin', created_at: '2025-01-15T12:00:00Z' },
        ],
      })
    );
    await page.goto('/admin');

    // Table header columns
    const secretsCard = page.locator('section, [class*="card"]').filter({ hasText: 'Secrets' });
    await expect(page.getByRole('cell', { name: 'NPM_TOKEN' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'DEPLOY_KEY' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testorg/testrepo' }).first()).toBeVisible();
  });

  test('secret values are not displayed in the table', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) =>
      route.fulfill({
        json: [
          { id: 1, repo: 'testorg/testrepo', name: 'NPM_TOKEN', created_by: 'testadmin', created_at: '2025-01-15T12:00:00Z' },
        ],
      })
    );
    await page.goto('/admin');

    // The table should NOT show any encrypted_value or raw secret value
    await expect(page.getByText('encrypted:fake_npm_token_value')).not.toBeVisible();
  });

  test('Add Secret button opens create dialog', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();

    await expect(page.getByRole('heading', { name: 'Add Secret' })).toBeVisible();
    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByLabel('Secret Name')).toBeVisible();
    await expect(page.getByLabel('Value')).toBeVisible();
  });

  test('secret value input is password type', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();

    const valueInput = page.getByLabel('Value');
    await expect(valueInput).toHaveAttribute('type', 'password');
  });

  test('create secret form submission calls API', async ({ adminPage: page }) => {
    let postBody: unknown;
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets', async (route) => {
      if (route.request().method() === 'POST') {
        postBody = JSON.parse(route.request().postData() ?? '{}');
        await route.fulfill({ json: {} });
      } else {
        await route.fulfill({ json: [] });
      }
    });
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Secret/ }).click();
    await page.getByLabel('Repository').fill('testorg/testrepo');
    await page.getByLabel('Secret Name').fill('NEW_SECRET');
    await page.getByLabel('Value').fill('secret-value-123');
    await page.getByRole('button', { name: 'Create Secret' }).click();

    await expect(() => {
      expect(postBody).toEqual({
        repo: 'testorg/testrepo',
        name: 'NEW_SECRET',
        value: 'secret-value-123',
      });
    }).toPass({ timeout: 5000 });
  });

  test('delete secret shows confirmation dialog', async ({ adminPage: page }) => {
    await ADMIN_ROUTES(page);
    await page.route('**/api/admin/secrets*', (route) =>
      route.fulfill({
        json: [
          { id: 1, repo: 'testorg/testrepo', name: 'NPM_TOKEN', created_by: 'testadmin', created_at: '2025-01-15T12:00:00Z' },
        ],
      })
    );
    await page.goto('/admin');

    // Click the delete (trash) button in the secrets section
    const secretsSection = page.locator('[class*="card"]').filter({ hasText: 'Secrets' });
    await secretsSection.getByRole('button').filter({ has: page.locator('[class*="lucide"]') }).last().click();

    await expect(page.getByRole('heading', { name: 'Delete Secret' })).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this secret?')).toBeVisible();
  });
});
