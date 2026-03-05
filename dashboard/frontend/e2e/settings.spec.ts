import { test, expect } from './fixtures';

test.describe('Settings page', () => {
  test('renders Settings heading', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });

  test('renders AI Provider card', async ({ adminPage: page }) => {
    await page.goto('/settings');

    // Scope to the personal AI Provider card (not the global one)
    const personalCard = page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();
    await expect(personalCard).toBeVisible();
  });

  test('shows provider radio options', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const personalCard = page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();
    await expect(personalCard.getByText('Anthropic (direct)')).toBeVisible();
    await expect(personalCard.getByText('OpenRouter', { exact: true })).toBeVisible();
  });

  test('API key input is password type', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const apiKeyInput = page.locator('#api-key');
    await expect(apiKeyInput).toHaveAttribute('type', 'password');
  });

  test('Save button is disabled when no API key and none stored', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const personalCard = page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();
    const saveButton = personalCard.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeDisabled();
  });

  test('Save button becomes enabled when API key is entered', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await page.locator('#api-key').fill('sk-test-12345');

    const personalCard = page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();
    const saveButton = personalCard.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeEnabled();
  });

  test('selecting OpenRouter shows model dropdown', async ({ adminPage: page }) => {
    await page.goto('/settings');

    // Click the OpenRouter radio in the personal card
    const personalCard = page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();
    await personalCard.locator('label').filter({ hasText: 'OpenRouter' }).click();

    await expect(page.locator('#model-select')).toBeVisible();
  });

  test('model dropdown is not shown for Anthropic provider', async ({ adminPage: page }) => {
    await page.goto('/settings');

    // Anthropic is selected by default — model dropdown should not be visible in personal card
    await expect(page.locator('#model-select')).not.toBeVisible();
  });
});

test.describe.serial('Settings - AI provider CRUD', () => {
  // Helper: scope to the personal AI Provider card
  const personalCard = (page: import('@playwright/test').Page) =>
    page.locator('[data-slot="card"]').filter({ hasText: 'AI Provider' }).first();

  // OpenRouter save + verify + disconnect
  test('save OpenRouter with specific model', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await card.locator('label').filter({ hasText: 'OpenRouter' }).click();

    const modelSelect = page.locator('#model-select');
    await expect(modelSelect).toBeVisible();
    await modelSelect.selectOption('openai/gpt-4o');

    await page.locator('#api-key').fill('sk-or-test-key-for-e2e');
    await card.getByRole('button', { name: 'Save' }).click();

    await expect(card.getByText('(API key stored)')).toBeVisible({ timeout: 10000 });
  });

  test('verify OpenRouter model persists after reload', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    const connected = card.getByText('Connected provider:').locator('..');
    await expect(connected).toBeVisible();
    await expect(connected.getByText('OpenRouter')).toBeVisible();
    await expect(connected.getByText('GPT-4o')).toBeVisible();
  });

  test('disconnect OpenRouter setting', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await card.getByRole('button', { name: 'Disconnect' }).click();

    await expect(card.getByRole('button', { name: 'Save' })).toBeVisible({ timeout: 10000 });
    await expect(card.getByText('(API key stored)')).not.toBeVisible();
  });

  // Anthropic save + verify + disconnect
  test('save Anthropic API key', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await page.locator('#api-key').fill('sk-ant-test-key-for-e2e');
    await card.getByRole('button', { name: 'Save' }).click();

    await expect(card.getByText('(API key stored)')).toBeVisible({ timeout: 10000 });
  });

  test('verify connected provider info persists', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await expect(card.getByText('Connected provider:')).toBeVisible();
    await expect(card.getByText('(API key stored)')).toBeVisible();
    await expect(card.getByRole('button', { name: 'Disconnect' })).toBeVisible();
    await expect(card.getByRole('button', { name: 'Update' })).toBeVisible();
  });

  test('API key label changes when key is stored', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await expect(card.getByText('API Key (leave blank to keep existing)')).toBeVisible();
  });

  test('disconnect removes API key', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await card.getByRole('button', { name: 'Disconnect' }).click();

    await expect(card.getByRole('button', { name: 'Save' })).toBeVisible({ timeout: 10000 });
    await expect(card.getByText('(API key stored)')).not.toBeVisible();
  });

  test('verify disconnected state persists', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = personalCard(page);
    await expect(card.getByRole('button', { name: 'Save' })).toBeVisible();
    await expect(card.getByRole('button', { name: 'Save' })).toBeDisabled();
    await expect(card.getByText('(API key stored)')).not.toBeVisible();
  });
});

test.describe('Settings - Global AI provider (admin)', () => {
  const globalCard = (page: import('@playwright/test').Page) =>
    page.locator('[data-slot="card"]').filter({ hasText: 'Organization AI Provider' });

  test('admin sees global AI provider card', async ({ adminPage: page }) => {
    await page.goto('/settings');

    await expect(globalCard(page)).toBeVisible();
    await expect(globalCard(page).getByText('fallback for users')).toBeVisible();
  });

  test('member does not see global AI provider card', async ({ memberPage: page }) => {
    await page.goto('/settings');

    await expect(page.locator('text=Organization AI Provider')).not.toBeVisible();
  });

  test('save and disconnect global API key', async ({ adminPage: page }) => {
    await page.goto('/settings');

    const card = globalCard(page);
    await page.locator('#global-api-key').fill('sk-global-test-key');
    await card.getByRole('button', { name: 'Save' }).click();

    await expect(card.getByText('(API key stored)')).toBeVisible({ timeout: 10000 });

    await card.getByRole('button', { name: 'Disconnect' }).click();
    await expect(card.getByText('(API key stored)')).not.toBeVisible({ timeout: 10000 });
  });
});
