import { test, expect, TEST_IDS } from './fixtures';

const setupAdminRoutes = async (page: import('@playwright/test').Page) => {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
  );
  await page.route('**/api/admin/users', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/admin/secrets*', (route) => route.fulfill({ json: [] }));
};

test.describe('Admin - Repo Policies section', () => {
  test('renders Repo Policies heading and Add Policy button', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Repo Policies' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Policy/ }).first()).toBeVisible();
  });

  test('shows empty state when no policies exist', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByText('No policies configured.')).toBeVisible();
  });

  test('renders policies table with data', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) =>
      route.fulfill({
        json: [
          {
            id: 1,
            repo: 'testorg/testrepo',
            protected_branches: ['main', 'master', 'production'],
            allowed_tools: null,
            network_egress: null,
            max_cost_usd: 10.0,
            require_approval_above_usd: 5.0,
            created_by: 'testadmin',
            created_at: '2025-01-01T00:00:00Z',
            updated_at: '2025-01-01T00:00:00Z',
          },
        ],
      })
    );
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByRole('cell', { name: 'testorg/testrepo' }).first()).toBeVisible();
    await expect(page.getByText('$10')).toBeVisible();
    await expect(page.getByText('main').first()).toBeVisible();
  });

  test('Add Policy opens create dialog with form fields', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();

    await expect(page.getByRole('heading', { name: 'Add Policy' })).toBeVisible();
    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByText('Protected Branches')).toBeVisible();
    await expect(page.getByText('Tool Restrictions')).toBeVisible();
    await expect(page.getByText('Network Egress')).toBeVisible();
    await expect(page.getByLabel('Max Cost (USD)')).toBeVisible();
    await expect(page.getByLabel('Require Approval Above (USD)')).toBeVisible();
  });

  test('create policy form submission calls API', async ({ adminPage: page }) => {
    let postBody: unknown;
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', async (route) => {
      if (route.request().method() === 'POST') {
        postBody = JSON.parse(route.request().postData() ?? '{}');
        await route.fulfill({
          json: {
            id: 2, repo: 'testorg/newrepo', protected_branches: ['main', 'master'],
            allowed_tools: null, network_egress: null, max_cost_usd: null,
            require_approval_above_usd: null, created_by: 'testadmin',
            created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z',
          },
        });
      } else {
        await route.fulfill({ json: [] });
      }
    });
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();
    await page.getByLabel('Repository').fill('testorg/newrepo');
    await page.getByRole('button', { name: 'Create Policy' }).click();

    await expect(() => {
      expect(postBody).toHaveProperty('repo', 'testorg/newrepo');
    }).toPass({ timeout: 5000 });
  });

  test('delete policy shows confirmation dialog', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) =>
      route.fulfill({
        json: [
          {
            id: 1, repo: 'testorg/testrepo', protected_branches: ['main'],
            allowed_tools: null, network_egress: null, max_cost_usd: null,
            require_approval_above_usd: null, created_by: 'testadmin',
            created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z',
          },
        ],
      })
    );
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    // Click the delete button in the policies section
    const policiesSection = page.locator('[class*="card"]').filter({ hasText: 'Repo Policies' });
    await policiesSection.getByRole('button').filter({ has: page.locator('[class*="lucide"]') }).last().click();

    await expect(page.getByRole('heading', { name: 'Delete Policy' })).toBeVisible();
    await expect(page.getByText('Are you sure you want to delete this policy?')).toBeVisible();
  });

  test('preset selector populates form fields', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) =>
      route.fulfill({
        json: [
          {
            name: 'strict',
            description: 'Strict security preset',
            protected_branches: ['main', 'release'],
            allowed_tools: { allow: ['Read', 'Grep'] },
            network_egress: null,
            max_cost_usd: 5.0,
            require_approval_above_usd: 2.0,
          },
        ],
      })
    );
    await page.goto('/admin');

    await page.getByRole('button', { name: /Add Policy/ }).first().click();

    // Select the preset
    const presetSelect = page.locator('select').filter({ hasText: 'Custom (no preset)' });
    await presetSelect.selectOption('strict');

    // Verify max cost is populated
    await expect(page.getByLabel('Max Cost (USD)')).toHaveValue('5');
    await expect(page.getByLabel('Require Approval Above (USD)')).toHaveValue('2');
  });
});

test.describe('Admin - Org Policies section', () => {
  test('renders Org Policies heading and Add Org Policy button', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(page.getByRole('heading', { name: 'Org Policies' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Add Org Policy/ })).toBeVisible();
  });

  test('shows org policy description text', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    await expect(
      page.getByText('Org policies apply as defaults to all repositories under the organization')
    ).toBeVisible();
  });

  test('renders org policies table with data', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) =>
      route.fulfill({
        json: [
          {
            id: 1, org: 'testorg', protected_branches: ['main', 'master'],
            allowed_tools: null, network_egress: null,
            max_cost_usd: 50.0, require_approval_above_usd: 20.0,
            created_by: 'testadmin', created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z',
          },
        ],
      })
    );
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await expect(orgSection.getByText('testorg')).toBeVisible();
    await expect(orgSection.getByText('$50')).toBeVisible();
  });

  test('delete org policy shows confirmation dialog', async ({ adminPage: page }) => {
    await setupAdminRoutes(page);
    await page.route('**/api/admin/policies', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/admin/org-policies', (route) =>
      route.fulfill({
        json: [
          {
            id: 1, org: 'testorg', protected_branches: ['main'],
            allowed_tools: null, network_egress: null,
            max_cost_usd: null, require_approval_above_usd: null,
            created_by: 'testadmin', created_at: '2025-01-01T00:00:00Z', updated_at: '2025-01-01T00:00:00Z',
          },
        ],
      })
    );
    await page.route('**/api/admin/policy-presets', (route) => route.fulfill({ json: [] }));
    await page.goto('/admin');

    const orgSection = page.locator('[class*="card"]').filter({ hasText: 'Org Policies' });
    await orgSection.getByRole('button').filter({ has: page.locator('[class*="lucide"]') }).last().click();

    await expect(page.getByRole('heading', { name: 'Delete Policy' })).toBeVisible();
  });
});
