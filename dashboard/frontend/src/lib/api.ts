export interface UserInfo {
  login: string;
  name: string;
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
export const listTasks = () => apiFetch<Task[]>('/api/tasks');
export const getTask = (id: string) => apiFetch<Task>(`/api/tasks/${id}`);
export const createTask = (data: { prompt: string; repo: string; base_branch?: string; image_urls?: string[] }) =>
  apiFetch<Task>('/api/tasks', {
    method: 'POST',
    body: JSON.stringify(data),
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
export const classifyPrompt = (prompt: string) =>
  apiFetch<{ repo: string }>('/api/classify', {
    method: 'POST',
    body: JSON.stringify({ prompt }),
  });

// WebSocket helpers
export function connectPreviewLogs(slug: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/logs/${slug}`);
}

export function connectTaskOutput(id: string): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${location.host}/api/ws/tasks/${id}`);
}
