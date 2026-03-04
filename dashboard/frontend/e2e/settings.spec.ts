import { test, expect } from './fixtures';

test.describe('Settings page', () => {
  test('renders Settings heading', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });

  test('renders AI Provider card', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByText('AI Provider')).toBeVisible();
  });

  test('shows provider radio options', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByText('Anthropic (direct)')).toBeVisible();
    await expect(page.getByText('OpenRouter', { exact: true })).toBeVisible();
  });

  test('API key input is password type', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const apiKeyInput = page.getByLabel(/API Key/);
    await expect(apiKeyInput).toHaveAttribute('type', 'password');
  });

  test('Save button is disabled when no API key and none stored', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const saveButton = page.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeDisabled();
  });

  test('Save button becomes enabled when API key is entered', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.getByLabel(/API Key/).fill('sk-test-12345');

    const saveButton = page.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeEnabled();
  });

  test('selecting OpenRouter shows model dropdown', async ({ adminPage: page }) => {
    await page.goto('/settings');

    // Click the OpenRouter radio button label
    await page.locator('label').filter({ hasText: 'OpenRouter' }).click();

    await expect(page.getByLabel('Model', { exact: true })).toBeVisible();
    await expect(page.locator('#model-select')).toBeVisible();
  });

  test('model dropdown is not shown for Anthropic provider', async ({ adminPage: page }) => {
    await page.goto('/settings');

    // Anthropic is selected by default
    await expect(page.getByLabel('Model')).not.toBeVisible();
  });
});

test.describe.serial('Settings - AI provider CRUD', () => {
  // OpenRouter save + verify + disconnect
  test('save OpenRouter with specific model', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.locator('label').filter({ hasText: 'OpenRouter' }).click();

    const modelSelect = page.locator('#model-select');
    await expect(modelSelect).toBeVisible();
    await modelSelect.selectOption('openai/gpt-4o');

    await page.getByLabel(/API Key/).fill('sk-or-test-key-for-e2e');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(page.getByText('(API key stored)')).toBeVisible({ timeout: 10000 });
  });

  test('verify OpenRouter model persists after reload', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const connected = page.getByText('Connected provider:').locator('..');
    await expect(connected).toBeVisible();
    await expect(connected.getByText('OpenRouter')).toBeVisible();
    await expect(connected.getByText('GPT-4o')).toBeVisible();
  });

  test('disconnect OpenRouter setting', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.getByRole('button', { name: 'Disconnect' }).click();

    await expect(page.getByRole('button', { name: 'Save' })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('(API key stored)')).not.toBeVisible();
  });

  // Anthropic save + verify + disconnect
  test('save Anthropic API key', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.getByLabel(/API Key/).fill('sk-ant-test-key-for-e2e');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(page.getByText('(API key stored)')).toBeVisible({ timeout: 10000 });
  });

  test('verify connected provider info persists', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByText('Connected provider:')).toBeVisible();
    await expect(page.getByText('(API key stored)')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Disconnect' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Update' })).toBeVisible();
  });

  test('API key label changes when key is stored', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByText('API Key (leave blank to keep existing)')).toBeVisible();
  });

  test('disconnect removes API key', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.getByRole('button', { name: 'Disconnect' }).click();

    await expect(page.getByRole('button', { name: 'Save' })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('(API key stored)')).not.toBeVisible();
  });

  test('verify disconnected state persists', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByRole('button', { name: 'Save' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Save' })).toBeDisabled();
    await expect(page.getByText('(API key stored)')).not.toBeVisible();
  });
});
