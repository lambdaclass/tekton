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

class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
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
    throw new ApiError(res.status, body.error || res.statusText);
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

export const listPolicies = () => apiFetch<RepoPolicy[]>('/api/admin/policies');
export const createPolicy = (data: { repo: string; protected_branches?: string[]; allowed_tools?: any; network_egress?: any; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<RepoPolicy>('/api/admin/policies', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
export const updatePolicy = (id: number, data: { protected_branches?: string[]; allowed_tools?: any; network_egress?: any; max_cost_usd?: number | null; require_approval_above_usd?: number | null }) =>
  apiFetch<RepoPolicy>(`/api/admin/policies/${id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
export const deletePolicy = (id: number) =>
  apiFetch<{ deleted: boolean }>(`/api/admin/policies/${id}`, { method: 'DELETE' });

// AI Settings
export const getAiSettings = () =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null }>('/api/settings/ai');
export const setAiSettings = (data: { provider: string; api_key?: string; model?: string }) =>
  apiFetch<{ provider: string | null; has_api_key: boolean; model: string | null }>('/api/settings/ai', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
export const deleteAiSettings = () =>
  apiFetch<{ deleted: boolean }>('/api/settings/ai', { method: 'DELETE' });

// WebSocket helpers
export function connectPreviewLogs(slug: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/logs/${slug}`);
}

export function connectTaskOutput(id: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/tasks/${id}`);
}
