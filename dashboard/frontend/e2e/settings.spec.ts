import { test, expect, TEST_IDS } from './fixtures';

test.describe('Settings page', () => {
  test('renders Settings heading', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });

  test('renders AI Provider card', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByRole('heading', { name: /AI Provider/ })).toBeVisible();
  });

  test('shows provider radio options', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByText('Anthropic (direct)')).toBeVisible();
    await expect(page.getByText('OpenRouter')).toBeVisible();
  });

  test('API key input is password type', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    const apiKeyInput = page.getByLabel(/API Key/);
    await expect(apiKeyInput).toHaveAttribute('type', 'password');
  });

  test('Save button is disabled when no API key and none stored', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    const saveButton = page.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeDisabled();
  });

  test('Save button becomes enabled when API key is entered', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    await page.getByLabel(/API Key/).fill('sk-test-12345');

    const saveButton = page.getByRole('button', { name: 'Save' });
    await expect(saveButton).toBeEnabled();
  });

  test('shows connected provider info when API key is stored', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByText('Connected provider:')).toBeVisible();
    await expect(page.getByText('Anthropic (direct)')).toBeVisible();
    await expect(page.getByText('(API key stored)')).toBeVisible();
  });

  test('Disconnect button is shown when API key is stored', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByRole('button', { name: 'Disconnect' })).toBeVisible();
  });

  test('Update button is shown instead of Save when API key is stored', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByRole('button', { name: 'Update' })).toBeVisible();
  });

  test('selecting OpenRouter shows model dropdown', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    // Click the OpenRouter radio
    await page.getByText('OpenRouter').click();

    await expect(page.getByLabel('Model')).toBeVisible();
    // Should contain model options
    await expect(page.locator('#model-select option').first()).toBeVisible();
  });

  test('model dropdown is not shown for Anthropic provider', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: null, has_api_key: false, model: null } })
    );
    await page.goto('/settings');

    // Anthropic is selected by default
    await expect(page.getByLabel('Model')).not.toBeVisible();
  });

  test('save calls PUT API with correct payload', async ({ adminPage: page }) => {
    let putBody: unknown;
    await page.route('**/api/settings/ai', async (route) => {
      if (route.request().method() === 'PUT') {
        putBody = JSON.parse(route.request().postData() ?? '{}');
        await route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } });
      } else {
        await route.fulfill({ json: { provider: null, has_api_key: false, model: null } });
      }
    });
    await page.goto('/settings');

    await page.getByLabel(/API Key/).fill('sk-test-key-12345');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(() => {
      expect(putBody).toEqual({
        provider: 'anthropic',
        api_key: 'sk-test-key-12345',
      });
    }).toPass({ timeout: 5000 });
  });

  test('disconnect calls DELETE API', async ({ adminPage: page }) => {
    let deleteRequested = false;
    await page.route('**/api/settings/ai', async (route) => {
      if (route.request().method() === 'DELETE') {
        deleteRequested = true;
        await route.fulfill({ json: { deleted: true } });
      } else {
        await route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } });
      }
    });
    await page.goto('/settings');

    await page.getByRole('button', { name: 'Disconnect' }).click();

    await expect(() => {
      expect(deleteRequested).toBe(true);
    }).toPass({ timeout: 5000 });
  });

  test('API key label changes when key is stored', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: 'anthropic', has_api_key: true, model: null } })
    );
    await page.goto('/settings');

    await expect(page.getByText('API Key (leave blank to keep existing)')).toBeVisible();
  });

  test('connected model label shown for OpenRouter', async ({ adminPage: page }) => {
    await page.route('**/api/settings/ai', (route) =>
      route.fulfill({ json: { provider: 'openrouter', has_api_key: true, model: 'anthropic/claude-sonnet-4.6' } })
    );
    await page.goto('/settings');

    await expect(page.getByText('Claude Sonnet 4.6 (recommended)')).toBeVisible();
  });
});
