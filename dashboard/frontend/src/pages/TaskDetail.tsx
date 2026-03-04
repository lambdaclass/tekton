import { useState, useCallback, useMemo } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  ChevronLeft,
  RotateCcw,
  GitPullRequest,
  ExternalLink,
  ShieldAlert,
  Activity,
  ScrollText,
  FileDiff,
  Info,
  GitBranch,
  DollarSign,
  Cpu,
  Image as ImageIcon,
  MessageSquare,
  CheckCircle,
} from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import TaskChat from '@/components/TaskChat';
import DiffViewer from '@/components/DiffViewer';
import ActivityTimeline from '@/components/ActivityTimeline';
import {
  getTask,
  listSubtasks,
  listTaskActions,
  getMe,
  parseImageUrls,
  reopenTask,
  createPR,
  getTaskDiff,
  sendTaskMessage,
} from '@/lib/api';
import type { TaskAction } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { statusVariant } from '@/lib/status';
import { formatCost, timeAgo } from '@/lib/utils';

const CHAT_STATUSES = ['awaiting_followup', 'running_claude', 'pushing', 'creating_preview'];

function defaultTab(status: string | undefined): string {
  if (!status) return 'logs';
  if (['running_claude', 'awaiting_followup'].includes(status)) return 'conversation';
  if (['completed', 'failed'].includes(status)) return 'diff';
  return 'logs';
}

export default function TaskDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [connected, setConnected] = useState(false);

  const { data: task } = useQuery({
    queryKey: ['task', id],
    queryFn: () => getTask(id!),
    enabled: !!id,
    refetchInterval: 3000,
  });

  const { data: subtasks } = useQuery({
    queryKey: ['subtasks', id],
    queryFn: () => listSubtasks(id!),
    enabled: !!id,
  });

  const { data: me } = useQuery({
    queryKey: ['me'],
    queryFn: getMe,
  });

  const { data: actions } = useQuery({
    queryKey: ['task-actions', id],
    queryFn: () => listTaskActions(id!),
    enabled: !!id,
    refetchInterval: 5000,
  });

  const { data: diffData } = useQuery({
    queryKey: ['task-diff', id],
    queryFn: () => getTaskDiff(id!),
    enabled: !!task?.branch_name,
    staleTime: 30_000,
  });

  const onConnectionChange = useCallback((c: boolean) => setConnected(c), []);

  const queryClient = useQueryClient();
  const reopenMutation = useMutation({
    mutationFn: () => reopenTask(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['task', id] });
    },
  });

  const prMutation = useMutation({
    mutationFn: () => createPR(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['task', id] });
    },
  });

  const markDoneMutation = useMutation({
    mutationFn: () => sendTaskMessage(id!, '__done__'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['task', id] });
    },
  });

  const isViewer = me?.role === 'viewer';
  const showChat = task && CHAT_STATUSES.includes(task.status) && me && !isViewer;
  const canReopen = task && !isViewer && (task.status === 'completed' || task.status === 'failed');
  const canCreatePR =
    task &&
    !isViewer &&
    task.branch_name &&
    !task.pr_url &&
    (task.status === 'completed' || task.status === 'awaiting_followup');

  const initialTab = useMemo(() => {
    const tab = defaultTab(task?.status);
    if (tab === 'conversation' && !showChat) return 'activity';
    return tab;
  }, [task?.status, showChat]);

  const policyViolations = actions?.filter((a) => a.action_type === 'policy_violation') ?? [];
  const imageUrls = task ? parseImageUrls(task.image_url) : [];

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* ===== Top bar: navigation + title ===== */}
      <div className="flex items-center gap-2 px-1 pb-2 shrink-0">
        <Button variant="ghost" size="icon-sm" onClick={() => navigate('/tasks')}>
          <ChevronLeft className="size-4" />
        </Button>
        <h1 className="text-lg font-semibold truncate max-w-sm">
          {task?.name || <span className="font-mono text-muted-foreground">{id?.slice(0, 8)}</span>}
        </h1>
        {task &&
          (() => {
            const sv = statusVariant(task.status);
            const StatusIcon = sv.icon;
            return (
              <Badge variant={sv.variant} className={sv.className}>
                {StatusIcon && <StatusIcon className={sv.spin ? 'animate-spin' : ''} />}
                {task.status.replace(/_/g, ' ')}
              </Badge>
            );
          })()}
        {task && CHAT_STATUSES.includes(task.status) && (
          <span className={`inline-flex items-center gap-1.5 text-xs ${connected ? 'text-emerald-400' : 'text-muted-foreground'}`}>
            <span className={`size-1.5 rounded-full ${connected ? 'bg-emerald-400 animate-pulse' : 'bg-muted-foreground'}`} />
            {connected ? 'Live' : 'Disconnected'}
          </span>
        )}

        {/* Right side actions */}
        <div className="ml-auto flex items-center gap-2">
          {task?.parent_task_id && (
            <Link
              to={`/tasks/${task.parent_task_id}`}
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              Parent: <span className="font-mono">{task.parent_task_id.slice(0, 8)}</span>
            </Link>
          )}
          {task?.preview_url && (
            <a href={task.preview_url} target="_blank" rel="noopener noreferrer">
              <Button size="sm" className="gap-1.5 bg-primary text-primary-foreground hover:bg-primary/90 shadow-sm">
                <ExternalLink className="size-3.5" />
                Preview
              </Button>
            </a>
          )}
          {task?.status === 'awaiting_followup' && !isViewer && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => markDoneMutation.mutate()}
              disabled={markDoneMutation.isPending}
              className="text-emerald-600 dark:text-emerald-400 border-emerald-500/30 hover:bg-emerald-500/10"
            >
              <CheckCircle className="size-3.5 mr-1" />
              {markDoneMutation.isPending ? 'Completing...' : 'Mark Done'}
            </Button>
          )}
          {canReopen && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => reopenMutation.mutate()}
              disabled={reopenMutation.isPending}
            >
              <RotateCcw className="size-3.5 mr-1" />
              {reopenMutation.isPending ? 'Reopening...' : 'Reopen'}
            </Button>
          )}
          {canCreatePR && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => prMutation.mutate()}
              disabled={prMutation.isPending}
            >
              <GitPullRequest className="size-3.5 mr-1" />
              {prMutation.isPending ? 'Creating...' : 'Create PR'}
            </Button>
          )}
          {task?.pr_url && (
            <a href={task.pr_url} target="_blank" rel="noopener noreferrer">
              <Button size="sm" variant="outline">
                <GitPullRequest className="size-3.5 mr-1" />
                PR #{task.pr_number}
              </Button>
            </a>
          )}
        </div>
      </div>

      {/* ===== Metadata bar ===== */}
      {task && (
        <div className="flex items-center gap-4 px-1 pb-3 text-xs text-muted-foreground border-b border-border/50 shrink-0">
          <span className="inline-flex items-center gap-1">
            <GitBranch className="size-3" />
            {task.repo}
            {task.branch_name && <span className="font-mono ml-1 text-foreground/60">({task.branch_name})</span>}
          </span>
          {task.total_cost_usd ? (
            <span className="inline-flex items-center gap-1 tabular-nums">
              <DollarSign className="size-3" />
              {formatCost(task.total_cost_usd)}
            </span>
          ) : null}
          {(task.total_input_tokens || task.total_output_tokens) ? (
            <span className="inline-flex items-center gap-1 tabular-nums">
              <Cpu className="size-3" />
              {(task.total_input_tokens ?? 0).toLocaleString()} in / {(task.total_output_tokens ?? 0).toLocaleString()} out
            </span>
          ) : null}
          <span className="ml-auto">{timeAgo(task.created_at)}</span>
        </div>
      )}

      {/* ===== Policy violations banner ===== */}
      {policyViolations.length > 0 && (
        <PolicyBanner violations={policyViolations} />
      )}

      {/* ===== Main content: full-width tabs ===== */}
      <Tabs defaultValue={initialTab} className="flex flex-col flex-1 min-h-0 pt-3">
        <TabsList variant="line" className="shrink-0 border-b border-border pb-0 mb-0">
          {showChat && (
            <TabsTrigger value="conversation" className="gap-1.5">
              <MessageSquare className="size-3.5" />
              Conversation
              {task?.status === 'awaiting_followup' && (
                <span className="size-1.5 rounded-full bg-amber-400 animate-pulse" />
              )}
            </TabsTrigger>
          )}
          <TabsTrigger value="activity" className="gap-1.5">
            <Activity className="size-3.5" />
            Activity
          </TabsTrigger>
          <TabsTrigger value="logs" className="gap-1.5">
            <ScrollText className="size-3.5" />
            Logs
          </TabsTrigger>
          <TabsTrigger value="diff" className="gap-1.5">
            <FileDiff className="size-3.5" />
            Diff
            {diffData?.diff && (
              <span className="ml-1 text-[10px] text-primary tabular-nums">
                {diffData.diff.split('\n').filter(l => l.startsWith('+') && !l.startsWith('+++')).length}+
              </span>
            )}
          </TabsTrigger>
          <TabsTrigger value="info" className="gap-1.5">
            <Info className="size-3.5" />
            Info
          </TabsTrigger>
        </TabsList>

        {/* Conversation tab */}
        {showChat && (
          <TabsContent value="conversation" className="flex-1 flex flex-col min-h-0 rounded-b-lg border border-t-0 border-border bg-card overflow-hidden">
            <div className="flex-1 flex flex-col min-h-0 max-w-3xl mx-auto w-full">
              <TaskChat
                taskId={id!}
                currentUserEmail={me!.login}
                taskStatus={task!.status}
              />
            </div>
          </TabsContent>
        )}

        {/* Activity tab — clean timeline only */}
        <TabsContent value="activity" className="flex-1 overflow-y-auto rounded-b-lg border border-t-0 border-border bg-card p-4">
          <ActivityTimeline actions={actions} />
        </TabsContent>

        {/* Logs tab — forceMount keeps the WebSocket alive across tab switches */}
        <TabsContent value="logs" forceMount className="flex-1 flex flex-col min-h-0 rounded-b-lg border border-t-0 border-border bg-card overflow-hidden data-[state=inactive]:hidden">
          <LogsTabs taskId={id!} onConnectionChange={onConnectionChange} />
        </TabsContent>

        {/* Diff tab */}
        <TabsContent value="diff" className="flex-1 overflow-y-auto rounded-b-lg border border-t-0 border-border bg-card">
          {diffData?.diff ? (
            <DiffViewer diff={diffData.diff} />
          ) : (
            <div className="flex flex-col items-center justify-center py-16 text-muted-foreground">
              <FileDiff className="size-8 mb-2 opacity-30" />
              <p className="text-sm">
                {task?.branch_name ? 'No diff available yet.' : 'No branch created yet.'}
              </p>
            </div>
          )}
        </TabsContent>

        {/* Info tab — prompt, subtasks, images, metadata */}
        <TabsContent value="info" className="flex-1 overflow-y-auto rounded-b-lg border border-t-0 border-border bg-card p-4">
          {/* Prompt */}
          {task && (
            <div className="mb-6">
              <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground mb-2">Prompt</h3>
              <div className="text-sm whitespace-pre-wrap leading-relaxed rounded-md bg-background/50 border border-border/50 p-3">
                {task.prompt}
              </div>
            </div>
          )}

          {/* Images */}
          {imageUrls.length > 0 && (
            <div className="mb-6">
              <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground mb-2 inline-flex items-center gap-1.5">
                <ImageIcon className="size-3" />
                Reference Images
              </h3>
              <div className="flex flex-wrap gap-2">
                {imageUrls.map((url, i) => (
                  <a key={i} href={url} target="_blank" rel="noopener noreferrer">
                    <img
                      src={url}
                      alt={`Reference ${i + 1}`}
                      className="max-h-48 rounded-md border border-border hover:border-muted-foreground/30 transition-colors"
                      style={{ objectFit: 'contain' }}
                    />
                  </a>
                ))}
              </div>
            </div>
          )}

          {/* Error message */}
          {task?.error_message && (
            <div className="mb-6 rounded-md border border-destructive/40 bg-destructive/5 px-4 py-3">
              <span className="text-destructive text-xs font-medium uppercase tracking-wider">Error</span>
              <p className="mt-1 text-sm text-destructive/80">{task.error_message}</p>
            </div>
          )}

          {/* Subtasks */}
          {subtasks && subtasks.length > 0 && (
            <div className="mb-6">
              <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground mb-2">
                Subtasks ({subtasks.length})
              </h3>
              <div className="space-y-2">
                {subtasks.map((sub) => {
                  const sv = statusVariant(sub.status);
                  const SubIcon = sv.icon;
                  return (
                    <Link key={sub.id} to={`/tasks/${sub.id}`}>
                      <div className="flex items-center gap-3 p-2.5 rounded-md border border-border hover:bg-secondary/40 transition-colors">
                        <Badge variant={sv.variant} className={`${sv.className} text-[10px]`}>
                          {SubIcon && <SubIcon className={`size-3 ${sv.spin ? 'animate-spin' : ''}`} />}
                          {sub.status.replace(/_/g, ' ')}
                        </Badge>
                        <span className="text-sm truncate flex-1">{sub.name || sub.prompt}</span>
                        <span className="font-mono text-xs text-muted-foreground">{sub.id.slice(0, 8)}</span>
                      </div>
                    </Link>
                  );
                })}
              </div>
            </div>
          )}

          {/* Task details */}
          {task && (
            <div>
              <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground mb-2">Details</h3>
              <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
                <dt className="text-muted-foreground">Task ID</dt>
                <dd className="font-mono text-xs">{task.id}</dd>
                <dt className="text-muted-foreground">Created by</dt>
                <dd>{task.created_by || '—'}</dd>
                <dt className="text-muted-foreground">Created</dt>
                <dd>{new Date(task.created_at).toLocaleString()}</dd>
                {task.updated_at && (
                  <>
                    <dt className="text-muted-foreground">Updated</dt>
                    <dd>{new Date(task.updated_at).toLocaleString()}</dd>
                  </>
                )}
                {task.agent_name && (
                  <>
                    <dt className="text-muted-foreground">Agent</dt>
                    <dd className="font-mono text-xs">{task.agent_name}</dd>
                  </>
                )}
              </dl>
            </div>
          )}
        </TabsContent>
      </Tabs>
    </div>
  );
}

/** Agent Logs / Container Logs sub-tabs inside the Logs tab */
function LogsTabs({
  taskId,
  onConnectionChange,
}: {
  taskId: string;
  onConnectionChange: (c: boolean) => void;
}) {
  const [logView, setLogView] = useState<'agent' | 'container'>('agent');

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div className="flex gap-1 border-b border-border/50 px-3 py-1.5 shrink-0 bg-card/30">
        <button
          className={`px-2.5 py-1 rounded text-xs font-medium transition-all duration-150 ${
            logView === 'agent'
              ? 'bg-secondary text-foreground'
              : 'text-muted-foreground hover:text-foreground hover:bg-muted/30'
          }`}
          onClick={() => setLogView('agent')}
        >
          Agent Logs
        </button>
        <button
          className={`px-2.5 py-1 rounded text-xs font-medium transition-all duration-150 ${
            logView === 'container'
              ? 'bg-secondary text-foreground'
              : 'text-muted-foreground hover:text-foreground hover:bg-muted/30'
          }`}
          onClick={() => setLogView('container')}
        >
          Container Logs
        </button>
      </div>
      <div className="flex-1 min-h-0">
        {logView === 'agent' ? (
          <LogViewer taskId={taskId} onConnectionChange={onConnectionChange} />
        ) : (
          <LogViewer previewSlug={`t-${taskId.slice(0, 6)}`} />
        )}
      </div>
    </div>
  );
}

function PolicyBanner({ violations }: { violations: TaskAction[] }) {
  return (
    <div className="my-2 flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-4 py-2.5 text-sm text-destructive shrink-0">
      <ShieldAlert className="size-4 mt-0.5 shrink-0" />
      <div>
        <span className="font-medium text-xs">
          {violations.length} policy violation{violations.length > 1 ? 's' : ''}
        </span>
        <ul className="mt-1 space-y-0.5 text-xs opacity-70">
          {violations.map((v) => (
            <li key={v.id}>
              {v.tool_name && <span className="font-medium">{v.tool_name}</span>}
              {v.summary && (
                <span> — {v.summary.replace(/^POLICY VIOLATION:\s*\S+\s*—\s*/, '')}</span>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
