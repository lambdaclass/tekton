import { test, expect } from './fixtures';

test.describe('Automated Previews page', () => {
  test('renders page heading', async ({ adminPage: page }) => {
    await page.goto('/webhooks');
    await expect(page.getByRole('heading', { name: 'Automated Previews' })).toBeVisible();
  });

  test('renders description text', async ({ adminPage: page }) => {
    await page.goto('/webhooks');
    await expect(
      page.getByText('preview environment is automatically created'),
    ).toBeVisible();
  });

  test('renders Repositories card with subtitle', async ({ adminPage: page }) => {
    await page.goto('/webhooks');
    await expect(page.getByText('Repositories', { exact: true })).toBeVisible();
    await expect(
      page.getByText('Only repositories where you have GitHub admin access'),
    ).toBeVisible();
  });

  test('shows error state when GitHub API is unreachable', async ({ adminPage: page }) => {
    await page.goto('/webhooks');
    await expect(page.getByText('Failed to load repositories.')).toBeVisible();
  });

  test('sidebar navigation links to webhooks page', async ({ adminPage: page }) => {
    await page.goto('/');
    await page.getByRole('link', { name: 'Automated Previews' }).click();
    await expect(page).toHaveURL('/webhooks');
  });

  test('viewer user gets rejected by API', async ({ viewerPage: page }) => {
    await page.goto('/webhooks');
    await expect(page.getByText('Failed to load repositories.')).toBeVisible();
  });
});
