import * as http from 'http';
import { test, expect } from './fixtures';

// playwright.config.ts points the backend's GITHUB_API_BASE_URL here
const GITHUB_PORT = 3298;

/** A stand-in for GitHub's repo endpoint, which the backend asks whether the user may push */
function startMockGitHub(): Promise<http.Server> {
  const permissions: Record<string, { pull: boolean; push: boolean }> = {
    '/repos/testorg/testrepo': { pull: true, push: true },
    '/repos/testorg/readonly': { pull: true, push: false },
  };
  const server = http.createServer((req, res) => {
    const perms = permissions[req.url ?? ''];
    res.setHeader('Content-Type', 'application/json');
    if (!req.headers.authorization || !perms) {
      res.statusCode = 404;
      res.end(JSON.stringify({ message: 'Not Found' }));
      return;
    }
    res.end(JSON.stringify({ permissions: perms }));
  });
  return new Promise((resolve) => server.listen(GITHUB_PORT, () => resolve(server)));
}

test.describe('Task Create', () => {
  test('New Task button toggles creation form', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    const newTaskBtn = adminPage.getByRole('button', { name: 'New Task' });
    await expect(newTaskBtn).toBeVisible();

    // Open the form
    await newTaskBtn.click();
    await expect(adminPage.getByText('New Task', { exact: false }).locator('visible=true').first()).toBeVisible();
    await expect(adminPage.getByLabel('Prompt')).toBeVisible();

    // Button now says Cancel
    await expect(adminPage.getByRole('button', { name: 'Cancel' })).toBeVisible();

    // Close the form
    await adminPage.getByRole('button', { name: 'Cancel' }).click();
    await expect(adminPage.getByLabel('Prompt')).toHaveCount(0);
  });

  test('form has required fields: prompt and repo', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByLabel('Prompt')).toBeVisible();
    await expect(adminPage.getByLabel('Repository')).toBeVisible();
  });

  test('form has base branch field', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByText('Base Branch')).toBeVisible();
  });

  test('form has optional branch name field', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByLabel('Branch Name (optional)')).toBeVisible();
    await expect(adminPage.getByPlaceholder('Auto-generated from task name if left blank')).toBeVisible();
  });

  test('form has image attachments section', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByText('Image Attachments')).toBeVisible();
    await expect(adminPage.getByText('Drop images here or click to select')).toBeVisible();
  });

  test('form has submit button', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByRole('button', { name: 'Submit Task' })).toBeVisible();
  });

  test('prompt field has placeholder text', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByPlaceholder('Describe the coding task...')).toBeVisible();
  });

  test('repo field has placeholder text', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await expect(adminPage.getByPlaceholder('owner/repo')).toBeVisible();
  });

  test('form submission sends correct API request', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    // Intercept the API call
    const createPromise = adminPage.waitForRequest((req) =>
      req.url().includes('/api/tasks') && req.method() === 'POST'
    );

    // Fill in the form
    await adminPage.getByLabel('Prompt').fill('Test task from e2e');
    await adminPage.getByLabel('Repository').fill('testorg/testrepo');

    // Submit
    await adminPage.getByRole('button', { name: 'Submit Task' }).click();

    const request = await createPromise;
    const body = request.postDataJSON();
    expect(body.prompt).toBe('Test task from e2e');
    expect(body.repo).toBe('testorg/testrepo');
  });

  test('form can fill in custom branch name', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    const branchInput = adminPage.getByLabel('Branch Name (optional)');
    await branchInput.fill('my-custom-branch');
    await expect(branchInput).toHaveValue('my-custom-branch');
  });

  test('viewer user does not see New Task button', async ({ viewerPage }) => {
    await viewerPage.goto('/tasks');
    await expect(viewerPage.getByRole('heading', { name: 'Tasks' })).toBeVisible();
    await expect(viewerPage.getByRole('button', { name: 'New Task' })).toHaveCount(0);
  });

  test('form prompt field is required', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    // The textarea has required attribute
    const promptField = adminPage.getByLabel('Prompt');
    await expect(promptField).toHaveAttribute('required', '');
  });

  test('form repo field is required', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    const repoField = adminPage.getByLabel('Repository');
    await expect(repoField).toHaveAttribute('required', '');
  });

  test('image upload shows preview in form', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    // Create a 1x1 red PNG in memory
    const pngBuffer = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==',
      'base64'
    );

    // Set the file on the hidden file input
    const fileInput = adminPage.locator('input[type="file"]');
    await fileInput.setInputFiles({
      name: 'test-image.png',
      mimeType: 'image/png',
      buffer: pngBuffer,
    });

    // An image preview should appear
    await expect(adminPage.locator('img[alt="Upload preview 1"]')).toBeVisible({ timeout: 5000 });
  });
});

test.describe.serial('Task Create - E2E', () => {
  let github: http.Server;
  test.beforeAll(async () => {
    github = await startMockGitHub();
  });
  test.afterAll(async () => {
    await new Promise((resolve) => github.close(resolve));
  });

  test('create task and verify it appears in task list', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await adminPage.getByLabel('Prompt').fill('E2E test task — automated creation test');
    await adminPage.getByLabel('Repository').fill('testorg/testrepo');
    await adminPage.getByRole('button', { name: 'Submit Task' }).click();

    // Form should close after submission
    await expect(adminPage.getByLabel('Prompt')).toHaveCount(0, { timeout: 10000 });

    // Task should appear in the list with pending status
    await expect(adminPage.getByText('E2E test task — automated creation test')).toBeVisible({ timeout: 10000 });
  });

  test('verify created task persists on page reload', async ({ adminPage }) => {
    await adminPage.goto('/tasks');

    await expect(adminPage.getByText('E2E test task — automated creation test')).toBeVisible();
  });

  test('refuses a repo the user cannot push to on GitHub', async ({ adminPage }) => {
    await adminPage.goto('/tasks');
    await adminPage.getByRole('button', { name: 'New Task' }).click();

    await adminPage.getByLabel('Prompt').fill('E2E task on a read-only repo');
    await adminPage.getByLabel('Repository').fill('testorg/readonly');
    await adminPage.getByRole('button', { name: 'Submit Task' }).click();

    await expect(adminPage.getByText(/can't push to 'testorg\/readonly'/)).toBeVisible({ timeout: 10000 });
    await expect(adminPage.getByLabel('Prompt')).toBeVisible();
  });
});
