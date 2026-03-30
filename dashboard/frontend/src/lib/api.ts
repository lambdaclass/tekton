export interface UserInfo {
  login: string;
  name: string;
  role: string;
}

export interface Preview {
  slug: string;
  repo: string;
  branch: string;
  url: string;
}

export interface Task {
  id: string;
  prompt: string;
  repo: string;
  base_branch: string;
  branch_name: string | null;
  agent_name: string | null;
  status: string;
  preview_slug: string | null;
  preview_url: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
  parent_task_id: string | null;
  created_by: string | null;
  screenshot_url: string | null;
  image_url: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  total_cost_usd: number | null;
  name: string | null;
  pr_url: string | null;
  pr_number: number | null;
}

export interface TaskMessage {
  id: number;
  task_id: string;
  sender: string;
  content: string;
  created_at: string;
  image_url: string | null;
}

export interface TaskLog {
  id: number;
  task_id: string;
  timestamp: string;
  line: string;
}

export interface TaskAction {
  id: number;
  task_id: string;
  action_type: string;
  tool_name: string | null;
  tool_input: unknown | null;
  summary: string | null;
  created_at: string;
}

export interface PaginatedTasks {
  tasks: Task[];
  total: number;
  page: number;
  per_page: number;
}

export interface ListTasksParams {
  status?: string;
  repo?: string;
  created_by?: string;
  search?: string;
  page?: number;
  per_page?: number;
}

export class ApiError extends Error {
  status: number;
  errorCode?: string;
  constructor(status: number, message: string, errorCode?: string) {
    super(message);
    this.status = status;
    this.errorCode = errorCode;
  }
}

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: 'include',
    ...opts,
    headers: {
      'Content-Type': 'application/json',
      ...opts?.headers,
    },
  });
  if (!res.ok) {
    if (res.status === 401) {
      throw new ApiError(401, 'Not authenticated');
    }
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new ApiError(res.status, body.error || res.statusText, body.error_code);
  }
  return res.json();
}

// Config
export interface PublicConfig {
  preview_domain: string;
  github_org: string;
}
export const getConfig = () => apiFetch<PublicConfig>('/api/config');

// Auth
export const getMe = () => apiFetch<UserInfo>('/api/auth/me');
export const logout = () => fetch('/api/auth/logout', { method: 'POST', credentials: 'include' });

// Previews
export const listPreviews = () => apiFetch<Preview[]>('/api/previews');
export const createPreview = (data: { repo: string; branch: string; slug?: string }) =>
  apiFetch<{ message: string; output: string }>('/api/previews', {
    method: 'POST',
    body: JSON.stringify(data),
  });
export const destroyPreview = (slug: string) =>
  apiFetch<{ message: string; output: string }>(`/api/previews/${slug}`, { method: 'DELETE' });
export const updatePreview = (slug: string) =>
  apiFetch<{ message: string; output: string }>(`/api/previews/${slug}/update`, { method: 'POST' });

// Uploads
export const uploadImage = async (file: File): Promise<{ url: string }> => {
  const formData = new FormData();
  formData.append('image', file);
  const res = await fetch('/api/uploads', { method: 'POST', body: formData, credentials: 'include' });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || 'Upload failed');
  }
  return res.json();
};

// Tasks
export const listTasks = (params?: ListTasksParams) => {
  const searchParams = new URLSearchParams();
  if (params?.status) searchParams.set('status', params.status);
  if (params?.repo) searchParams.set('repo', params.repo);
  if (params?.created_by) searchParams.set('created_by', params.created_by);
  if (params?.search) searchParams.set('search', params.search);
  if (params?.page) searchParams.set('page', String(params.page));
  if (params?.per_page) searchParams.set('per_page', String(params.per_page));
  const qs = searchParams.toString();
  return apiFetch<PaginatedTasks>(`/api/tasks${qs ? `?${qs}` : ''}`);
};
export const getTask = (id: string) => apiFetch<Task>(`/api/tasks/${id}`);
export const createTask = (data: { prompt: string; repo: string; base_branch?: string; image_urls?: string[]; custom_branch_name?: string }) =>
  apiFetch<Task>('/api/tasks', {
    method: 'POST',
    body: JSON.stringify(data),
  });
export const updateTaskName = (id: string, name: string) =>
  apiFetch<Task>(`/api/tasks/${id}/name`, {
    method: 'PATCH',
    body: JSON.stringify({ name }),
  });
export const getTaskLogs = (id: string) => apiFetch<TaskLog[]>(`/api/tasks/${id}/logs`);
export const listSubtasks = (id: string) => apiFetch<Task[]>(`/api/tasks/${id}/subtasks`);
export const listTaskMessages = (id: string) => apiFetch<TaskMessage[]>(`/api/tasks/${id}/messages`);
export const sendTaskMessage = (id: string, content: string, image_urls?: string[]) =>
  apiFetch<TaskMessage>(`/api/tasks/${id}/messages`, {
    method: 'POST',
    body: JSON.stringify({ content, image_urls }),
  });
export const reopenTask = (id: string) =>
  apiFetch<Task>(`/api/tasks/${id}/reopen`, { method: 'POST' });
export const listTaskActions = (id: string) =>
  apiFetch<TaskAction[]>(`/api/tasks/${id}/actions`);
export const createPR = (id: string) =>
  apiFetch<Task>(`/api/tasks/${id}/create-pr`, { method: 'POST' });
export const linkPR = (id: string, pr_url: string) =>
  apiFetch<Task>(`/api/tasks/${id}/link-pr`, {
    method: 'POST',
    body: JSON.stringify({ pr_url }),
  });

/** Parse image_url JSON column (stored as JSON array string) into an array of URLs. */
export function parseImageUrls(raw: string | null | undefined): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    // Backwards compat: single URL string
    return [raw];
  }
}
export const getTaskDiff = (id: string) =>
  apiFetch<{ diff: string }>(`/api/tasks/${id}/diff`);
export const listRepos = () => apiFetch<string[]>('/api/repos');
export const listBranches = (owner: string, repo: string) =>
  apiFetch<string[]>(`/api/repos/${owner}/${repo}/branches`);

// Admin
export const listUsers = () =>
  apiFetch<{ login: string; name: string; role: string }[]>('/api/admin/users');
export const setUserRole = (login: string, role: string) =>
  apiFetch<{ login: string; role: string }>(`/api/admin/users/${encodeURIComponent(login)}/role`, {
    method: 'PUT',
    body: JSON.stringify({ role }),
  });
export const getUserRepos = (login: string) =>
  apiFetch<string[]>(`/api/admin/users/${encodeURIComponent(login)}/repos`);
export const setUserRepos = (login: string, repos: string[]) =>
  apiFetch<string[]>(`/api/admin/users/${encodeURIComponent(login)}/repos`, {
    method: 'PUT',
    body: JSON.stringify({ repos }),
  });
export const listSecrets = (repo?: string) => {
  const qs = repo ? `?repo=${encodeURIComponent(repo)}` : '';
  return apiFetch<{ id: number; repo: string; name: string; created_by: string | null; created_at: string }[]>(
    `/api/admin/secrets${qs}`,
  );
};
export const createSecret = (data: { repo: string; name: string; value: string }) =>
  apiFetch<unknown>('/api/admin/secrets', { method: 'POST', body: JSON.stringify(data) });
export const deleteSecret = (id: number) =>
  apiFetch<{ ok: boolean }>(`/api/admin/secrets/${id}`, { method: 'DELETE' });

export interface RepoPolicy {
  id: number;
  repo: string;
  protected_branches: string[];
  allowed_tools: { allow?: string[]; deny?: string[] } | null;
  network_egress: { allowlist?: string[]; denylist?: string[]; default?: string } | null;
  max_cost_usd: number | null;
  require_approval_above_usd: number | null;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export type AllowedToolsValue = { allow?: string[]; deny?: string[] } | null;
export type NetworkEgressValue = { allowlist?: string[]; denylist?: string[]; default?: string } | null;

export const listPolicies = () => apiFetch<RepoPolicy[]>('/api/admin/policies');
export const createPolicy = (data: { repo: string; protected_branches?: string[]; allowed_tools?: AllowedToolsValue; network_egress?: NetworkEgressValue; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<RepoPolicy>('/api/admin/policies', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
export const updatePolicy = (id: number, data: { protected_branches?: string[]; allowed_tools?: AllowedToolsValue; network_egress?: NetworkEgressValue; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<RepoPolicy>(`/api/admin/policies/${id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
export const deletePolicy = (id: number) =>
  apiFetch<{ deleted: boolean }>(`/api/admin/policies/${id}`, { method: 'DELETE' });

export interface OrgPolicy {
  id: number;
  org: string;
  protected_branches: string[];
  allowed_tools: { allow?: string[]; deny?: string[] } | null;
  network_egress: { allowlist?: string[]; denylist?: string[]; default?: string } | null;
  max_cost_usd: number | null;
  require_approval_above_usd: number | null;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export const listOrgPolicies = () => apiFetch<OrgPolicy[]>('/api/admin/org-policies');
export const createOrgPolicy = (data: { org: string; protected_branches?: string[]; allowed_tools?: AllowedToolsValue; network_egress?: NetworkEgressValue; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<OrgPolicy>('/api/admin/org-policies', { method: 'POST', body: JSON.stringify(data) });
export const updateOrgPolicy = (id: number, data: { protected_branches?: string[]; allowed_tools?: AllowedToolsValue; network_egress?: NetworkEgressValue; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<OrgPolicy>(`/api/admin/org-policies/${id}`, { method: 'PUT', body: JSON.stringify(data) });
export const deleteOrgPolicy = (id: number) =>
  apiFetch<{ deleted: boolean }>(`/api/admin/org-policies/${id}`, { method: 'DELETE' });

export interface PolicyPreset {
  name: string;
  description: string;
  protected_branches: string[];
  allowed_tools: { allow?: string[]; deny?: string[] } | null;
  network_egress: { allowlist?: string[]; denylist?: string[]; allow?: string[] } | null;
  max_cost_usd: number | null;
  require_approval_above_usd: number | null;
}

export const listPresets = () => apiFetch<PolicyPreset[]>('/api/admin/policy-presets');
export const applyPreset = (data: { preset: string; repo?: string; org?: string }) =>
  apiFetch<{ preset: string; repo_policy?: RepoPolicy; org_policy?: OrgPolicy }>('/api/admin/policies/from-preset', {
    method: 'POST',
    body: JSON.stringify(data),
  });

// Cost tracking
export interface CostSummary {
  total_cost_usd: number;
  total_tasks: number;
  avg_cost_per_task: number;
  total_input_tokens: number;
  total_output_tokens: number;
}
export interface CostTrend {
  day: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_compute_seconds: number;
  cost_usd: number;
  task_count: number;
}
export interface CostGroupRow {
  group_key: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_compute_seconds: number;
  cost_usd: number;
}
export interface Budget {
  id: number;
  scope_type: string;
  scope: string;
  monthly_limit_usd: number;
  alert_threshold_pct: number;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}
export const getCostSummary = (days?: number) => {
  const qs = days ? `?days=${days}` : '';
  return apiFetch<CostSummary>(`/api/admin/cost/summary${qs}`);
};
export const getCostTrends = (days?: number) => {
  const qs = days ? `?days=${days}` : '';
  return apiFetch<CostTrend[]>(`/api/admin/cost/trends${qs}`);
};
export const getCostByUser = (days?: number) => {
  const qs = days ? `?days=${days}` : '';
  return apiFetch<CostGroupRow[]>(`/api/admin/cost/by-user${qs}`);
};
export const getCostByRepo = (days?: number) => {
  const qs = days ? `?days=${days}` : '';
  return apiFetch<CostGroupRow[]>(`/api/admin/cost/by-repo${qs}`);
};
export const listBudgets = () => apiFetch<Budget[]>('/api/admin/budgets');
export const createBudget = (data: { scope_type: string; scope: string; monthly_limit_usd: number; alert_threshold_pct: number }) =>
  apiFetch<Budget>('/api/admin/budgets', { method: 'POST', body: JSON.stringify(data) });
export const deleteBudget = (id: number) =>
  apiFetch<{ deleted: boolean }>(`/api/admin/budgets/${id}`, { method: 'DELETE' });

// Audit log
export interface AuditLogEntry {
  id: number;
  event_type: string;
  actor: string;
  target: string | null;
  detail: Record<string, unknown> | null;
  created_at: string;
}
export interface PaginatedAuditLog {
  entries: AuditLogEntry[];
  total: number;
  page: number;
  per_page: number;
}
export interface AuditLogParams {
  event_type?: string;
  actor?: string;
  target?: string;
  start_date?: string;
  end_date?: string;
  page?: number;
  per_page?: number;
}
export const getAuditLog = (params?: AuditLogParams) => {
  const sp = new URLSearchParams();
  if (params?.event_type) sp.set('event_type', params.event_type);
  if (params?.actor) sp.set('actor', params.actor);
  if (params?.target) sp.set('target', params.target);
  if (params?.start_date) sp.set('start_date', params.start_date);
  if (params?.end_date) sp.set('end_date', params.end_date);
  if (params?.page) sp.set('page', String(params.page));
  if (params?.per_page) sp.set('per_page', String(params.per_page));
  const qs = sp.toString();
  return apiFetch<PaginatedAuditLog>(`/api/admin/audit-log${qs ? `?${qs}` : ''}`);
};

// AI Settings
export const getAiSettings = () =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null; has_global_fallback?: boolean }>('/api/settings/ai');
export const setAiSettings = (data: { provider: string; api_key?: string; model?: string }) =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null }>('/api/settings/ai', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
export const deleteAiSettings = () =>
  apiFetch<{ deleted: boolean }>('/api/settings/ai', { method: 'DELETE' });

// Admin: Global AI Settings
export const getGlobalAiSettings = () =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null }>('/api/admin/settings/ai');
export const setGlobalAiSettings = (data: { provider: string; api_key?: string; model?: string }) =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null }>('/api/admin/settings/ai', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
export const deleteGlobalAiSettings = () =>
  apiFetch<{ deleted: boolean }>('/api/admin/settings/ai', { method: 'DELETE' });

// Intake Sources
export interface IntakeSource {
  id: number;
  name: string;
  provider: string;
  enabled: boolean;
  config: Record<string, unknown>;
  target_repo: string;
  target_base_branch: string;
  label_filter: string[];
  prompt_template: string | null;
  run_as_user: string;
  poll_interval_secs: number;
  max_concurrent_tasks: number;
  max_tasks_per_poll: number;
  auto_create_pr: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface IntakeIssue {
  id: number;
  source_id: number;
  external_id: string;
  external_url: string | null;
  external_title: string;
  external_body: string | null;
  external_labels: string[];
  external_updated_at: string | null;
  task_id: string | null;
  status: string;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export interface IntakePollLog {
  id: number;
  source_id: number;
  polled_at: string;
  issues_found: number;
  issues_created: number;
  issues_skipped: number;
  error_message: string | null;
  duration_ms: number | null;
}

export const listIntakeSources = () => apiFetch<IntakeSource[]>('/api/admin/intake/sources');
export const createIntakeSource = (data: {
  name: string; provider: string; target_repo: string;
  target_base_branch?: string; label_filter?: string[]; prompt_template?: string;
  run_as_user: string; poll_interval_secs?: number; max_concurrent_tasks?: number;
  max_tasks_per_poll?: number; auto_create_pr?: boolean;
  config?: Record<string, unknown>;
}) => apiFetch<IntakeSource>('/api/admin/intake/sources', { method: 'POST', body: JSON.stringify(data) });
export const updateIntakeSource = (id: number, data: Record<string, unknown>) =>
  apiFetch<IntakeSource>(`/api/admin/intake/sources/${id}`, { method: 'PUT', body: JSON.stringify(data) });
export const deleteIntakeSource = (id: number) =>
  apiFetch<{ deleted: boolean }>(`/api/admin/intake/sources/${id}`, { method: 'DELETE' });
export const toggleIntakeSource = (id: number) =>
  apiFetch<IntakeSource>(`/api/admin/intake/sources/${id}/toggle`, { method: 'POST' });
export const listIntakeIssues = (sourceId: number) =>
  apiFetch<IntakeIssue[]>(`/api/admin/intake/sources/${sourceId}/issues`);
export const listIntakeLogs = (sourceId: number) =>
  apiFetch<IntakePollLog[]>(`/api/admin/intake/sources/${sourceId}/logs`);
export const testPollSource = (id: number) =>
  apiFetch<{ title: string; url: string; labels: string[] }[]>(`/api/admin/intake/sources/${id}/test`, { method: 'POST' });

// Intake Board (all issues across sources)
export interface IntakeIssueWithMeta extends IntakeIssue {
  source_name: string;
  source_repo: string;
  task_status: string | null;
}

export const listAllIntakeIssues = () =>
  apiFetch<IntakeIssueWithMeta[]>('/api/admin/intake/issues');

export const updateIntakeIssueStatus = (id: number, status: string) =>
  apiFetch<IntakeIssueWithMeta>(`/api/admin/intake/issues/${id}/status`, {
    method: 'PATCH',
    body: JSON.stringify({ status }),
  });

// Webhooks
export interface RepoWebhookInfo {
  full_name: string;
  hook_id: number | null;
  active: boolean;
}
export const listRepoWebhooks = () => apiFetch<RepoWebhookInfo[]>('/api/webhooks/repos');
export const createRepoWebhook = (owner: string, repo: string) =>
  apiFetch<RepoWebhookInfo>(`/api/webhooks/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`, { method: 'POST' });
export const deleteRepoWebhook = (owner: string, repo: string, hookId: number) =>
  apiFetch<{ deleted: boolean }>(`/api/webhooks/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/${hookId}`, { method: 'DELETE' });

// WebSocket helpers
export function connectPreviewLogs(slug: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/logs/${slug}`);
}

export function connectTaskOutput(id: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/tasks/${id}`);
}
