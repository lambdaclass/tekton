import { test, expect } from './fixtures';

test.describe('Theme Toggle', () => {
  test('theme toggle button is visible in sidebar', async ({ adminPage }) => {
    await adminPage.goto('/');
    const toggle = adminPage.getByRole('button', { name: /light mode|dark mode/i });
    await expect(toggle).toBeVisible();
  });

  test('clicking toggle switches to dark mode', async ({ adminPage }) => {
    await adminPage.goto('/');
    // Click the moon icon button (switches to dark)
    await adminPage.getByRole('button', { name: 'Dark mode' }).click();
    // html element should have 'dark' class
    await expect(adminPage.locator('html')).toHaveClass(/dark/);
  });

  test('clicking toggle again switches back to light mode', async ({ adminPage }) => {
    await adminPage.goto('/');
    await adminPage.getByRole('button', { name: 'Dark mode' }).click();
    await expect(adminPage.locator('html')).toHaveClass(/dark/);
    await adminPage.getByRole('button', { name: 'Light mode' }).click();
    await expect(adminPage.locator('html')).not.toHaveClass(/dark/);
  });
});
