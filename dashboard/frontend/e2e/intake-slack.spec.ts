import * as http from 'http';
import { APIRequestContext } from '@playwright/test';
import { test, expect } from './fixtures';

// playwright.config.ts points the backend's SLACK_API_URL here
const SLACK_PORT = 3299;
const SOURCE = 'Slack Notify';
const CHANNEL = 'C0E2ETEST';
const TOKEN = 'xoxb-e2e-token';

interface SlackCall {
  method: string;
  auth: string | undefined;
  body: { channel: string; text: string; ts?: string; thread_ts?: string };
}

/** A stand-in for the Slack Web API that records every call */
function startMockSlack(calls: SlackCall[]): Promise<http.Server> {
  let nextTs = 1;
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (chunk) => (raw += chunk));
    req.on('end', () => {
      const method = (req.url ?? '').replace(/^\//, '');
      calls.push({ method, auth: req.headers.authorization, body: JSON.parse(raw || '{}') });
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ ok: true, ts: `1700000000.00000${nextTs++}` }));
    });
  });
  return new Promise((resolve) => server.listen(SLACK_PORT, () => resolve(server)));
}

async function notify(request: APIRequestContext) {
  const resp = await request.post('/api/test/intake/slack-notify');
  expect(resp.status()).toBe(204);
}

async function setIssue(request: APIRequestContext, id: number, data: object) {
  const resp = await request.patch(`/api/test/intake/issues/${id}/status`, { data });
  expect(resp.status()).toBe(204);
}

async function setTask(request: APIRequestContext, id: string, data: object) {
  const resp = await request.patch(`/api/test/tasks/${id}/status`, { data });
  expect(resp.status()).toBe(204);
}

test.describe('Intake Slack notifications', () => {
  test.describe.configure({ mode: 'serial' });

  const calls: SlackCall[] = [];
  let server: http.Server;
  let request: APIRequestContext;
  let sourceId: number;
  let issueId: number;

  test.beforeAll(async ({ browser }) => {
    server = await startMockSlack(calls);
    const context = await browser.newContext({ storageState: './e2e/.auth/admin.json' });
    request = context.request;

    const sources: { id: number; name: string }[] = await (
      await request.get('/api/admin/intake/sources')
    ).json();
    sourceId = sources.find((s) => s.name === SOURCE)!.id;
    const issues: { id: number; external_title: string }[] = await (
      await request.get('/api/admin/intake/issues')
    ).json();
    issueId = issues.find((i) => i.external_title === 'Slack test issue')!.id;
  });

  test.afterAll(async () => {
    await new Promise((resolve) => server.close(resolve));
  });

  test('a channel needs a bot token', async () => {
    const resp = await request.put(`/api/admin/intake/sources/${sourceId}`, {
      data: { slack_channel_id: CHANNEL },
    });
    expect(resp.status()).toBe(400);
  });

  test('turning Slack on does not announce work already running', async () => {
    const resp = await request.put(`/api/admin/intake/sources/${sourceId}`, {
      data: { slack_channel_id: CHANNEL, slack_bot_token: TOKEN },
    });
    expect(resp.ok()).toBeTruthy();
    const source = await resp.json();
    expect(source.slack_channel_id).toBe(CHANNEL);
    expect(source.has_slack_token).toBe(true);
    expect(JSON.stringify(source)).not.toContain(TOKEN);

    await notify(request);
    expect(calls).toEqual([]);
  });

  test('picking an issue up posts a message to the channel', async () => {
    await setIssue(request, issueId, { status: 'task_created', task_id: 'task-slack-1' });
    await notify(request);

    expect(calls).toHaveLength(1);
    const [call] = calls;
    expect(call.method).toBe('chat.postMessage');
    expect(call.auth).toBe(`Bearer ${TOKEN}`);
    expect(call.body.channel).toBe(CHANNEL);
    expect(call.body.thread_ts).toBeUndefined();
    expect(call.body.text).toContain('*In progress*');
    expect(call.body.text).toContain('<https://github.com/testorg/slackrepo/issues/911|Slack test issue>');
    expect(call.body.text).toContain('<http://localhost:3200/tasks/task-slack-1|task>');

    // Nothing changed, nothing to say
    await notify(request);
    expect(calls).toHaveLength(1);
  });

  test('PR and review are replies in the thread, and the parent is updated', async () => {
    calls.length = 0;
    await setTask(request, 'task-slack-1', {
      status: 'pending',
      pr_url: 'https://github.com/testorg/slackrepo/pull/77',
      pr_number: 77,
    });
    await setIssue(request, issueId, { status: 'review' });
    await notify(request);

    const thread = '1700000000.000001';
    expect(calls.map((c) => c.method)).toEqual([
      'chat.update',
      'chat.postMessage',
      'chat.postMessage',
    ]);
    expect(calls[0].body.ts).toBe(thread);
    expect(calls[0].body.text).toContain('*Ready for review*');
    expect(calls[0].body.text).toContain('<https://github.com/testorg/slackrepo/pull/77|PR #77>');
    expect(calls[1].body.thread_ts).toBe(thread);
    expect(calls[1].body.text).toContain('Opened <https://github.com/testorg/slackrepo/pull/77|PR #77>');
    expect(calls[2].body.thread_ts).toBe(thread);
    expect(calls[2].body.text).toContain('Ready for review');
  });

  test('done is announced once', async () => {
    calls.length = 0;
    await setIssue(request, issueId, { status: 'done' });
    await notify(request);
    await notify(request);

    expect(calls.map((c) => c.method)).toEqual(['chat.update', 'chat.postMessage']);
    expect(calls[0].body.text).toContain('*Done*');
    expect(calls[1].body.text).toContain('Done');
  });

  test('the admin page shows the Slack setup', async ({ adminPage: page }) => {
    await page.goto('/admin');
    const section = page.locator('[class*="card"]').filter({ hasText: 'Intake Sources' });
    const row = section.locator('tr', { hasText: SOURCE });
    await expect(row.getByText('Slack', { exact: true })).toBeVisible();

    await row.getByTitle('Edit').click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByLabel('Slack Channel ID')).toHaveValue(CHANNEL);
    await expect(dialog.getByLabel('Slack Bot Token')).toHaveAttribute(
      'placeholder',
      'Leave empty to keep the current token',
    );
  });

  test('clearing the channel turns Slack off and drops the token', async () => {
    const resp = await request.put(`/api/admin/intake/sources/${sourceId}`, {
      data: { slack_channel_id: '' },
    });
    expect(resp.ok()).toBeTruthy();
    const source = await resp.json();
    expect(source.slack_channel_id).toBeNull();
    expect(source.has_slack_token).toBe(false);
  });
});
