import { test, expect } from './fixtures';

test.describe('Previews page', () => {
  test('renders page heading', async ({ adminPage: page }) => {
    await page.goto('/previews');
    await expect(page.getByRole('heading', { name: 'Previews' })).toBeVisible();
  });

  test('shows Create Preview button', async ({ adminPage: page }) => {
    await page.goto('/previews');
    await expect(page.getByRole('button', { name: 'Create Preview' })).toBeVisible();
  });

  test('shows empty/error state when no previews exist', async ({ adminPage: page }) => {
    await page.goto('/previews');
    // The preview binary doesn't exist in test env, so API returns error
    // and component shows "No active previews." since data is undefined
    await expect(page.getByText('No active previews.')).toBeVisible();
  });

  test('clicking Create Preview shows the form', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();

    await expect(page.getByText('New Preview')).toBeVisible();
    await expect(page.getByLabel('Repository')).toBeVisible();
    await expect(page.getByLabel('Branch')).toBeVisible();
    await expect(page.getByLabel('Slug (optional)')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create' })).toBeVisible();
  });

  test('clicking Cancel hides the create form', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    await expect(page.getByText('New Preview')).toBeVisible();

    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByText('New Preview')).not.toBeVisible();
  });

  test('create preview form slug field is editable', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    const slugInput = page.getByLabel('Slug (optional)');
    await slugInput.fill('my-custom-slug');
    await expect(slugInput).toHaveValue('my-custom-slug');
  });

  test('create preview form repo and branch are required', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    await expect(page.getByLabel('Repository')).toHaveAttribute('required', '');
    await expect(page.getByLabel('Branch')).toHaveAttribute('required', '');
  });

  test('create preview form submission shows error', async ({ adminPage: page }) => {
    await page.goto('/previews');

    await page.getByRole('button', { name: 'Create Preview' }).click();
    await page.getByLabel('Repository').fill('testorg/testrepo');
    await page.getByLabel('Branch').fill('feature-branch');
    await page.getByRole('button', { name: 'Create' }).click();

    // The dashboard first checks with the user's GitHub token that they can read the repo.
    // The seeded token is fake, so GitHub rejects it (or, offline, the request fails); either
    // way the form must show the error.
    await expect(page.getByText(/sign in again|failed|error/i).first()).toBeVisible({ timeout: 15000 });
  });
});
