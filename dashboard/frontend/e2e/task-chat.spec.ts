import { test, expect, TEST_IDS } from './fixtures';

test.describe.serial('Task Chat', () => {
  // Read-only tests first — these don't change task state

  test('shows Conversation tab for awaiting task', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(adminPage.getByRole('tab', { name: 'Conversation' })).toBeVisible();
  });

  test('renders seeded messages', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    await expect(adminPage.getByText('fixed the button alignment')).toBeVisible();
    await expect(adminPage.getByText('center it vertically')).toBeVisible();
  });

  test('renders system message with spinner', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    await expect(adminPage.getByText('Claude is thinking...')).toBeVisible();
  });

  test('shows Mark Done button', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(adminPage.getByRole('button', { name: 'Mark Done' })).toBeVisible();
  });

  test('send button is disabled when input is empty', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    const sendBtn = adminPage.locator('button[type="submit"]');
    await expect(sendBtn).toBeDisabled();
  });

  test('shows Preview tab for task with preview URL', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(adminPage.getByRole('tab', { name: 'Preview' })).toBeVisible();
  });

  test('shows "You" label on own message', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();
    // Logged in as testadmin, so "testadmin" sender should show "You"
    await expect(adminPage.getByText('You').first()).toBeVisible();
  });

  test('viewer cannot see Conversation tab', async ({ viewerPage }) => {
    await viewerPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await expect(viewerPage.getByRole('tab', { name: 'Conversation' })).toHaveCount(0);
  });

  // Destructive tests last — these send messages that may transition task state

  test('typing and sending a message posts it', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);
    await adminPage.getByRole('tab', { name: 'Conversation' }).click();

    const requestPromise = adminPage.waitForRequest((req) =>
      req.url().includes(`/api/tasks/${TEST_IDS.tasks.awaiting}/messages`) && req.method() === 'POST'
    );

    await adminPage.getByPlaceholder('Send a follow-up message...').fill('Please also fix padding');
    await adminPage.locator('button[type="submit"]').click();

    const request = await requestPromise;
    const body = request.postDataJSON();
    expect(body.content).toBe('Please also fix padding');
  });

  test('Mark Done sends __done__ message', async ({ adminPage }) => {
    await adminPage.goto(`/tasks/${TEST_IDS.tasks.awaiting}`);

    const requestPromise = adminPage.waitForRequest((req) =>
      req.url().includes(`/api/tasks/${TEST_IDS.tasks.awaiting}/messages`) && req.method() === 'POST'
    );

    await adminPage.getByRole('button', { name: 'Mark Done' }).click();

    const request = await requestPromise;
    const body = request.postDataJSON();
    expect(body.content).toBe('__done__');
  });
});
