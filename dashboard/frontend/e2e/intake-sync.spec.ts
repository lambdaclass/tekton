import { test, expect, TEST_IDS } from './fixtures';
import { APIRequestContext } from '@playwright/test';

const SYNC = TEST_IDS.intake.sync;

// Helper: set a task's status via the test-only endpoint
async function setTaskStatus(request: APIRequestContext, taskId: string, status: string) {
  const resp = await request.patch(`/api/test/tasks/${taskId}/status`, {
    data: { status },
  });
  expect(resp.status()).toBe(204);
}

// Helper: set an intake issue's status via the test-only endpoint
async function setIssueStatus(request: APIRequestContext, issueId: number, status: string) {
  const resp = await request.patch(`/api/test/intake/issues/${issueId}/status`, {
    data: { status },
  });
  expect(resp.status()).toBe(204);
}

// Helper: trigger sync via the test-only endpoint
async function triggerSync(request: APIRequestContext) {
  const resp = await request.post('/api/test/intake/sync');
  expect(resp.status()).toBe(204);
}

// Helper: get an intake issue by its external_title
async function getIssue(request: APIRequestContext, title: string) {
  const resp = await request.get('/api/admin/intake/issues');
  expect(resp.ok()).toBeTruthy();
  const issues: { id: number; external_title: string; status: string; error_message: string | null }[] =
    await resp.json();
  const issue = issues.find((i) => i.external_title === title);
  expect(issue, `Issue "${title}" not found`).toBeDefined();
  return issue!;
}

test.describe('Intake status sync', () => {
  test.describe.configure({ mode: 'serial' });

  let request: APIRequestContext;
  let sync1Id: number;
  let sync2Id: number;

  test.beforeAll(async ({ browser }) => {
    const context = await browser.newContext({
      storageState: './e2e/.auth/admin.json',
    });
    request = context.request;

    // Grab the DB IDs for our sync issues so we can reset them
    const issue1 = await getIssue(request, SYNC.issueSync1Title);
    const issue2 = await getIssue(request, SYNC.issueSync2Title);
    sync1Id = issue1.id;
    sync2Id = issue2.id;
  });

  test('task_created → review when task is awaiting_followup', async () => {
    await setTaskStatus(request, SYNC.taskSync1, 'awaiting_followup');
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync1Title);
    expect(issue.status).toBe('review');
  });

  test('review → task_created when task goes back to running_claude', async () => {
    // SYNC-1 is now review (from previous test)
    await setTaskStatus(request, SYNC.taskSync1, 'running_claude');
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync1Title);
    expect(issue.status).toBe('task_created');
  });

  test('task_created → failed when task fails', async () => {
    // SYNC-1 is now task_created (from previous test)
    await setTaskStatus(request, SYNC.taskSync1, 'failed');
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync1Title);
    expect(issue.status).toBe('failed');
    expect(issue.error_message).toBe('Linked task failed');
  });

  test('review → done when task is completed', async () => {
    // SYNC-2 starts as review, task-sync-2 starts as running_claude
    await setTaskStatus(request, SYNC.taskSync2, 'completed');
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync2Title);
    expect(issue.status).toBe('done');
  });

  test('task_created → review → done in single sync (two-step)', async () => {
    // Reset SYNC-1 to task_created and set its task to completed.
    // Sync query order: (1) task_created→review if completed, (3) review→done if completed.
    // Both fire in one sync call, so the issue should end up in done.
    await setIssueStatus(request, sync1Id, 'task_created');
    await setTaskStatus(request, SYNC.taskSync1, 'completed');
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync1Title);
    expect(issue.status).toBe('done');
  });

  test('no-op for backlog issues without tasks', async () => {
    await triggerSync(request);

    const issue = await getIssue(request, SYNC.issueSync3Title);
    expect(issue.status).toBe('backlog');
  });

  test.afterAll(async () => {
    // Reset to original seed state
    await setTaskStatus(request, SYNC.taskSync1, 'running_claude');
    await setTaskStatus(request, SYNC.taskSync2, 'running_claude');
    await setIssueStatus(request, sync1Id, 'task_created');
    await setIssueStatus(request, sync2Id, 'review');
  });
});
