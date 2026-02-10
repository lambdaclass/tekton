export type SessionStatus =
  | "pending"
  | "creating_container"
  | "cloning"
  | "running"
  | "pushing"
  | "done"
  | "failed";

export interface Repo {
  id: number;
  name: string;
  clone_url: string;
  description: string | null;
  default_branch: string;
}

export interface Session {
  id: string;
  repo_id: number;
  container_name: string | null;
  container_ip: string | null;
  branch_name: string | null;
  pr_url: string | null;
  status: SessionStatus;
  error_message: string | null;
  slack_channel: string | null;
  slack_thread_ts: string | null;
  task: string;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: number;
  session_id: string;
  role: "user" | "assistant" | "system";
  content: string;
  created_at: string;
}

export interface ContainerInfo {
  name: string;
  ip: string;
}

export interface StreamEvent {
  type: "text" | "tool_use" | "tool_result" | "completion" | "error";
  content: string;
  tool?: string;
  filePath?: string;
}

export interface SessionCallbacks {
  onStatusChange: (status: SessionStatus, detail?: string) => Promise<void>;
  onStreamEvent: (event: StreamEvent) => Promise<void>;
  onComplete: (prUrl: string) => Promise<void>;
  onError: (error: string) => Promise<void>;
}
