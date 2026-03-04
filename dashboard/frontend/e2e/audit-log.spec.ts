import { test, expect, TEST_IDS } from './fixtures';

const setupRoutes = async (page: import('@playwright/test').Page) => {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { login: TEST_IDS.users.admin, name: 'Test Admin', role: 'admin' } })
  );
};

const SAMPLE_ENTRIES = [
  { id: 1, event_type: 'auth.login', actor: 'testadmin', target: null, detail: { role: 'admin' }, created_at: '2025-01-15T10:00:00Z' },
  { id: 2, event_type: 'task.create', actor: 'testadmin', target: 'task-completed-1', detail: { repo: 'testorg/testrepo' }, created_at: '2025-01-15T11:00:00Z' },
  { id: 3, event_type: 'task.complete', actor: 'system', target: 'task-completed-1', detail: { cost_usd: 1.50 }, created_at: '2025-01-15T12:00:00Z' },
  { id: 4, event_type: 'admin.role_change', actor: 'testadmin', target: 'testviewer', detail: { new_role: 'viewer' }, created_at: '2025-01-15T13:00:00Z' },
];

test.describe('Audit Log', () => {
  test('renders Audit Log heading', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: [], total: 0, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible();
  });

  test('renders filter controls', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: [], total: 0, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    await expect(page.getByText('Event Type')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by actor...')).toBeVisible();
    await expect(page.getByPlaceholder('Filter by target...')).toBeVisible();
    await expect(page.getByText('Start Date')).toBeVisible();
    await expect(page.getByText('End Date')).toBeVisible();
  });

  test('shows empty state when no events exist', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: [], total: 0, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    await expect(page.getByText('No events found.')).toBeVisible();
  });

  test('renders audit log entries in table', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: SAMPLE_ENTRIES, total: 4, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    // Table headers
    await expect(page.getByRole('columnheader', { name: 'Timestamp' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Event Type' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Actor' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Target' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Detail' })).toBeVisible();

    // Entry data
    await expect(page.getByText('auth.login')).toBeVisible();
    await expect(page.getByText('task.create')).toBeVisible();
    await expect(page.getByText('task.complete')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'testadmin' }).first()).toBeVisible();
  });

  test('total count is displayed', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: SAMPLE_ENTRIES, total: 4, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    await expect(page.getByText('(4 total)')).toBeVisible();
  });

  test('detail expand/collapse toggle works', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({ json: { entries: SAMPLE_ENTRIES, total: 4, page: 1, per_page: 25 } })
    );
    await page.goto('/audit');

    // Click the first "Show" button to expand details
    const showButton = page.getByRole('button', { name: 'Show' }).first();
    await showButton.click();

    // The detail JSON should be visible in a pre block
    await expect(page.locator('pre').first()).toBeVisible();

    // Now it should say "Hide"
    const hideButton = page.getByRole('button', { name: 'Hide' }).first();
    await hideButton.click();

    // Pre block should not be visible
    await expect(page.locator('pre')).not.toBeVisible();
  });

  test('task target is a clickable link', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({
        json: {
          entries: [
            { id: 2, event_type: 'task.create', actor: 'testadmin', target: 'task-completed-1', detail: null, created_at: '2025-01-15T11:00:00Z' },
          ],
          total: 1,
          page: 1,
          per_page: 25,
        },
      })
    );
    await page.goto('/audit');

    const targetLink = page.getByRole('link', { name: 'task-completed-1' });
    await expect(targetLink).toBeVisible();
    await expect(targetLink).toHaveAttribute('href', '/tasks/task-completed-1');
  });

  test('pagination is shown when total exceeds per_page', async ({ adminPage: page }) => {
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', (route) =>
      route.fulfill({
        json: { entries: SAMPLE_ENTRIES, total: 50, page: 1, per_page: 25 },
      })
    );
    await page.goto('/audit');

    await expect(page.getByText('Page 1 of 2')).toBeVisible();
    await expect(page.getByRole('button', { name: /Prev/ })).toBeDisabled();
    await expect(page.getByRole('button', { name: /Next/ })).toBeEnabled();
  });

  test('Next button sends page parameter to API', async ({ adminPage: page }) => {
    let lastRequestUrl = '';
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', async (route) => {
      lastRequestUrl = route.request().url();
      await route.fulfill({
        json: { entries: SAMPLE_ENTRIES, total: 50, page: 1, per_page: 25 },
      });
    });
    await page.goto('/audit');

    await page.getByRole('button', { name: /Next/ }).click();

    await expect(() => {
      expect(lastRequestUrl).toContain('page=2');
    }).toPass({ timeout: 5000 });
  });

  test('actor filter sends filter parameter to API', async ({ adminPage: page }) => {
    let lastRequestUrl = '';
    await setupRoutes(page);
    await page.route('**/api/admin/audit-log*', async (route) => {
      lastRequestUrl = route.request().url();
      await route.fulfill({
        json: { entries: [], total: 0, page: 1, per_page: 25 },
      });
    });
    await page.goto('/audit');

    await page.getByPlaceholder('Filter by actor...').fill('testadmin');

    await expect(() => {
      expect(lastRequestUrl).toContain('actor=testadmin');
    }).toPass({ timeout: 5000 });
  });

  test('non-admin user is redirected away from audit log', async ({ memberPage: page }) => {
    await page.route('**/api/auth/me', (route) =>
      route.fulfill({ json: { login: TEST_IDS.users.member, name: 'Test Member', role: 'member' } })
    );
    await page.goto('/audit');
    await expect(page).toHaveURL('/');
  });
});
