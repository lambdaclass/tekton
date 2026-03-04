import { test, expect, TEST_IDS } from './fixtures';

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
});
