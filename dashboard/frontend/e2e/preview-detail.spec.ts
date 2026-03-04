import { test, expect } from './fixtures';

test.describe('Preview detail page', () => {
  test('shows preview slug in heading', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    await expect(page.getByRole('heading', { name: 'my-preview' })).toBeVisible();
  });

  test('shows back navigation to previews list', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    const backButton = page.getByRole('button', { name: /Previews/ });
    await expect(backButton).toBeVisible();
  });

  test('shows connection status badge (Disconnected by default)', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    // WebSocket will fail naturally in test env
    await expect(page.getByText('Disconnected')).toBeVisible();
  });

  test('shows Open Preview link with correct URL', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    const openLink = page.getByRole('link', { name: /Open Preview/ });
    await expect(openLink).toBeVisible();
    await expect(openLink).toHaveAttribute('href', 'https://my-preview.preview.test.dev');
  });

  test('shows Live Logs section', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    await expect(page.getByText('Live Logs')).toBeVisible();
  });

  test('back button navigates to previews list', async ({ adminPage: page }) => {
    await page.goto('/previews/my-preview');

    await page.getByRole('button', { name: /Previews/ }).click();
    await expect(page).toHaveURL(/\/previews$/);
  });
});
