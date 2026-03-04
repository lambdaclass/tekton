import { test, expect, TEST_IDS } from './fixtures';

test.describe('Previews page', () => {
  test('renders page heading', async ({ adminPage: page }) => {
    await page.goto('/previews');
    await expect(page.getByRole('heading', { name: 'Previews' })).toBeVisible();
  });

  test('shows Create Preview button', async ({ adminPage: page }) => {
    await page.goto('/previews');
    await expect(page.getByRole('button', { name: 'Create Preview' })).toBeVisible();
  });

  test('shows loading state initially', async ({ adminPage: page }) => {
    // Intercept the API to delay response
    await page.route('**/api/previews', async (route) => {
      await new Promise((r) => setTimeout(r, 500));
      await route.fulfill({ json: [] });
    });
    await page.goto('/previews');
    await expect(page.getByText('Loading previews...')).toBeVisible();
  });

  test('shows empty state when no previews exist', async ({ adminPage: page }) => {
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: [] })
    );
    await page.goto('/previews');
    await expect(page.getByText('No active previews.')).toBeVisible();
  });

  test('renders preview list with seeded data', async ({ adminPage: page }) => {
    const previews = [
      { slug: 'preview-1', repo: 'testorg/testrepo', branch: 'main', url: 'https://preview-1.test.dev' },
      { slug: 'preview-2', repo: 'testorg/frontend', branch: 'feat/new', url: 'https://preview-2.test.dev' },
    ];
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: previews })
    );
    await page.goto('/previews');

    await expect(page.getByText('preview-1')).toBeVisible();
    await expect(page.getByText('preview-2')).toBeVisible();
    await expect(page.getByText('testorg/testrepo')).toBeVisible();
    await expect(page.getByText('testorg/frontend')).toBeVisible();
  });

  test('each preview card shows slug, repo, and branch info', async ({ adminPage: page }) => {
    const previews = [
      { slug: 'my-preview', repo: 'testorg/testrepo', branch: 'feature-branch', url: 'https://my-preview.test.dev' },
    ];
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: previews })
    );
    await page.goto('/previews');

    await expect(page.getByText('my-preview')).toBeVisible();
    // The repo/branch info is shown as "repo / branch"
    await expect(page.getByText('testorg/testrepo / feature-branch')).toBeVisible();
  });

  test('preview card has Open, Logs, and Destroy buttons', async ({ adminPage: page }) => {
    const previews = [
      { slug: 'my-preview', repo: 'testorg/testrepo', branch: 'main', url: 'https://my-preview.test.dev' },
    ];
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: previews })
    );
    await page.goto('/previews');

    await expect(page.getByRole('link', { name: 'Open' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Logs' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Destroy' })).toBeVisible();
  });

  test('Open link points to preview URL', async ({ adminPage: page }) => {
    const previews = [
      { slug: 'my-preview', repo: 'testorg/testrepo', branch: 'main', url: 'https://my-preview.test.dev' },
    ];
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: previews })
    );
    await page.goto('/previews');

    const openLink = page.getByRole('link', { name: 'Open' });
    await expect(openLink).toHaveAttribute('href', 'https://my-preview.test.dev');
    await expect(openLink).toHaveAttribute('target', '_blank');
  });

  test('Logs link navigates to preview detail page', async ({ adminPage: page }) => {
    const previews = [
      { slug: 'my-preview', repo: 'testorg/testrepo', branch: 'main', url: 'https://my-preview.test.dev' },
    ];
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: previews })
    );
    await page.goto('/previews');

    const logsLink = page.getByRole('link', { name: 'Logs' });
    await expect(logsLink).toHaveAttribute('href', '/previews/my-preview');
  });

  test('clicking Create Preview shows the form', async ({ adminPage: page }) => {
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: [] })
    );
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();

    await expect(page.getByRole('heading', { name: 'New Preview' })).toBeVisible();
    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByLabel('Branch')).toBeVisible();
    await expect(page.getByLabel('Slug (optional)')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create' })).toBeVisible();
  });

  test('clicking Cancel hides the create form', async ({ adminPage: page }) => {
    await page.route('**/api/previews', (route) =>
      route.fulfill({ json: [] })
    );
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    await expect(page.getByRole('heading', { name: 'New Preview' })).toBeVisible();

    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByRole('heading', { name: 'New Preview' })).not.toBeVisible();
  });

  test('create preview form submission calls API', async ({ adminPage: page }) => {
    let requestBody: unknown;
    await page.route('**/api/previews', async (route) => {
      if (route.request().method() === 'POST') {
        requestBody = JSON.parse(route.request().postData() ?? '{}');
        await route.fulfill({ json: { message: 'Created', output: '' } });
      } else {
        await route.fulfill({ json: [] });
      }
    });
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    await page.getByLabel('Repository').fill('testorg/testrepo');
    await page.getByLabel('Branch').fill('feature-branch');
    await page.getByLabel('Slug (optional)').fill('my-custom-slug');
    await page.getByRole('button', { name: 'Create' }).click();

    await expect(() => {
      expect(requestBody).toEqual({
        repo: 'testorg/testrepo',
        branch: 'feature-branch',
        slug: 'my-custom-slug',
      });
    }).toPass({ timeout: 5000 });
  });
});
