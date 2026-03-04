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
} from '@/lib/api';
import type { TaskAction } from '@/lib/api';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { statusVariant } from '@/lib/status';
import { formatCost } from '@/lib/utils';

const CHAT_STATUSES = ['awaiting_followup', 'running_claude', 'pushing', 'creating_preview'];

function defaultTab(status: string | undefined): string {
  if (!status) return 'activity';
  if (['running_claude', 'creating_agent', 'cloning'].includes(status)) return 'activity';
  if (['completed', 'failed'].includes(status)) return 'diff';
  if (['pushing', 'creating_preview'].includes(status)) return 'logs';
  return 'activity';
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

  const isViewer = me?.role === 'viewer';
  const showChat = task && CHAT_STATUSES.includes(task.status) && me && !isViewer;
  const canReopen = task && !isViewer && (task.status === 'completed' || task.status === 'failed');
  const canCreatePR =
    task &&
    !isViewer &&
    task.branch_name &&
    !task.pr_url &&
    (task.status === 'completed' || task.status === 'awaiting_followup');

  const initialTab = useMemo(() => defaultTab(task?.status), [task?.status]);

  const policyViolations = actions?.filter((a) => a.action_type === 'policy_violation') ?? [];

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* ===== Header bar ===== */}
      <div className="flex flex-wrap items-center gap-3 px-1 pb-4 shrink-0 glass-card rounded-lg p-4">
        <Button variant="ghost" size="sm" onClick={() => navigate('/tasks')}>
          <ChevronLeft className="size-4" />
          Tasks
        </Button>
        <h1 className="text-2xl font-bold truncate max-w-md">
          {task?.name || <span className="font-mono">{id?.slice(0, 8)}</span>}
        </h1>
        {task &&
          (() => {
            const sv = statusVariant(task.status);
            const StatusIcon = sv.icon;
            return (
              <Badge variant={sv.variant} className={sv.className}>
                {StatusIcon && <StatusIcon className={sv.spin ? 'animate-spin' : ''} />}
                {task.status}
              </Badge>
            );
          })()}
        <Badge variant={connected ? 'default' : 'outline'}>
          {connected ? 'Live' : 'Disconnected'}
        </Badge>

        {/* Parent task link */}
        {task?.parent_task_id && (
          <Link
            to={`/tasks/${task.parent_task_id}`}
            className="text-xs font-mono text-blue-400 hover:text-blue-300"
          >
            Parent: {task.parent_task_id.slice(0, 8)}
          </Link>
        )}

        {/* Preview badge */}
        {task?.preview_url && (
          <a href={task.preview_url} target="_blank" rel="noopener noreferrer">
            <Badge variant="outline" className="gap-1 cursor-pointer hover:bg-muted">
              <ExternalLink className="size-3" />
              {task.preview_slug}
            </Badge>
          </a>
        )}

        {/* Metadata summary */}
        {task && (
          <div className="hidden md:flex items-center gap-3 text-xs text-muted-foreground ml-auto">
            <span>{task.repo}</span>
            {task.branch_name && <span className="font-mono">{task.branch_name}</span>}
            {task.total_cost_usd ? <span>{formatCost(task.total_cost_usd)}</span> : null}
            {(task.total_input_tokens || task.total_output_tokens) ? (
              <span>
                {(task.total_input_tokens ?? 0).toLocaleString()} / {(task.total_output_tokens ?? 0).toLocaleString()} tokens
              </span>
            ) : null}
          </div>
        )}

        {/* Action buttons */}
        <div className="flex items-center gap-2">
          {canReopen && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => reopenMutation.mutate()}
              disabled={reopenMutation.isPending}
            >
              <RotateCcw className="size-4 mr-1" />
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
              <GitPullRequest className="size-4 mr-1" />
              {prMutation.isPending ? 'Creating PR...' : 'Create PR'}
            </Button>
          )}
          {task?.pr_url && (
            <a href={task.pr_url} target="_blank" rel="noopener noreferrer">
              <Button size="sm" variant="outline">
                <ExternalLink className="size-4 mr-1" />
                View PR #{task.pr_number}
              </Button>
            </a>
          )}
        </div>
      </div>

      {/* ===== Policy violations banner ===== */}
      {policyViolations.length > 0 && (
        <PolicyBanner violations={policyViolations} />
      )}

      {/* ===== Split pane ===== */}
      <div
        className={`grid flex-1 min-h-0 gap-4 ${
          showChat
            ? 'grid-cols-1 md:grid-cols-[2fr_3fr]'
            : 'grid-cols-1'
        }`}
      >
        {/* Left pane: Chat */}
        {showChat && (
          <div className="h-[calc(100vh-10rem)] md:h-auto overflow-hidden rounded-lg border-r border-border/50 bg-card">
            <TaskChat
              taskId={id!}
              currentUserEmail={me!.login}
              previewUrl={task.preview_url ?? undefined}
              taskStatus={task.status}
            />
          </div>
        )}

        {/* Right pane: Tabbed workspace */}
        <Tabs defaultValue={initialTab} className="flex flex-col min-h-0 h-[calc(100vh-10rem)] md:h-auto">
          <TabsList className="shrink-0">
            <TabsTrigger value="activity">
              <Activity className="size-4" />
              Activity
            </TabsTrigger>
            <TabsTrigger value="logs">
              <ScrollText className="size-4" />
              Logs
            </TabsTrigger>
            <TabsTrigger value="diff">
              <FileDiff className="size-4" />
              Diff
            </TabsTrigger>
          </TabsList>

          {/* Activity tab */}
          <TabsContent value="activity" className="flex-1 overflow-y-auto mt-0 rounded-lg border border-border bg-card p-4">
            {/* Subtasks section */}
            {subtasks && subtasks.length > 0 && (
              <div className="mb-6">
                <h3 className="text-sm font-medium mb-2">Subtasks</h3>
                <div className="space-y-2">
                  {subtasks.map((sub) => (
                    <Link key={sub.id} to={`/tasks/${sub.id}`}>
                      <Card className="hover:border-muted-foreground/25 transition-colors">
                        <CardContent className="py-3">
                          <div className="flex items-center justify-between mb-1">
                            <span className="font-mono text-sm text-muted-foreground">
                              {sub.id.slice(0, 8)}
                            </span>
                            {(() => {
                              const sv = statusVariant(sub.status);
                              const SubIcon = sv.icon;
                              return (
                                <Badge variant={sv.variant} className={sv.className}>
                                  {SubIcon && (
                                    <SubIcon className={sv.spin ? 'animate-spin' : ''} />
                                  )}
                                  {sub.status}
                                </Badge>
                              );
                            })()}
                          </div>
                          <p className="text-sm line-clamp-1">{sub.prompt}</p>
                        </CardContent>
                      </Card>
                    </Link>
                  ))}
                </div>
              </div>
            )}

            {/* Prompt */}
            {task && (
              <div className="mb-6">
                <h3 className="text-sm font-medium mb-1 text-muted-foreground">Prompt</h3>
                <p className="text-sm whitespace-pre-wrap">{task.prompt}</p>
                {parseImageUrls(task.image_url).length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-2">
                    {parseImageUrls(task.image_url).map((url, i) => (
                      <a key={i} href={url} target="_blank" rel="noopener noreferrer">
                        <img
                          src={url}
                          alt={`Task reference image ${i + 1}`}
                          className="max-w-full rounded-md border border-border hover:opacity-90 transition-opacity"
                          style={{ maxHeight: '200px', objectFit: 'contain' }}
                        />
                      </a>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* Error message */}
            {task?.error_message && (
              <div className="mb-6 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3">
                <span className="text-destructive text-sm font-medium">Error</span>
                <p className="mt-1 text-sm text-destructive">{task.error_message}</p>
              </div>
            )}

            {/* Screenshot */}
            {task?.screenshot_url && (
              <div className="mb-6">
                <h3 className="text-sm font-medium mb-1 text-muted-foreground">Preview Screenshot</h3>
                <a href={task.screenshot_url} target="_blank" rel="noopener noreferrer">
                  <img
                    src={task.screenshot_url}
                    alt="Preview screenshot"
                    className="max-w-full rounded-md border border-border hover:opacity-90 transition-opacity"
                    style={{ maxHeight: '300px', objectFit: 'contain' }}
                  />
                </a>
              </div>
            )}

            {/* Timeline */}
            <h3 className="text-sm font-medium mb-2">Timeline</h3>
            <ActivityTimeline actions={actions} />
          </TabsContent>

          {/* Logs tab */}
          <TabsContent value="logs" className="flex-1 flex flex-col min-h-0 mt-0 rounded-lg border border-border bg-card overflow-hidden">
            <LogsTabs taskId={id!} onConnectionChange={onConnectionChange} />
          </TabsContent>

          {/* Diff tab */}
          <TabsContent value="diff" className="flex-1 overflow-y-auto mt-0 rounded-lg border border-border bg-card">
            {diffData?.diff ? (
              <DiffViewer diff={diffData.diff} />
            ) : (
              <p className="p-4 text-sm text-muted-foreground">
                {task?.branch_name ? 'No diff available yet.' : 'No branch created yet.'}
              </p>
            )}
          </TabsContent>
        </Tabs>
      </div>
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
      <div className="flex gap-1 border-b border-border px-3 py-1.5 shrink-0">
        <button
          className={`px-2 py-1 rounded text-xs font-medium transition-colors ${
            logView === 'agent'
              ? 'bg-muted text-foreground'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          onClick={() => setLogView('agent')}
        >
          Agent Logs
        </button>
        <button
          className={`px-2 py-1 rounded text-xs font-medium transition-colors ${
            logView === 'container'
              ? 'bg-muted text-foreground'
              : 'text-muted-foreground hover:text-foreground'
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
    <div className="mb-4 flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm text-destructive shrink-0">
      <ShieldAlert className="size-4 mt-0.5 shrink-0" />
      <div>
        <span className="font-medium">
          {violations.length} policy violation{violations.length > 1 ? 's' : ''} detected
        </span>
        <ul className="mt-1 space-y-0.5 text-xs opacity-80">
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
