import { useState, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { ChevronLeft, RotateCcw, GitPullRequest, ExternalLink } from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import TaskChat from '@/components/TaskChat';
import DiffViewer from '@/components/DiffViewer';
import { getTask, listSubtasks, getMe, parseImageUrls, reopenTask, createPR, getTaskDiff } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import { statusVariant } from '@/lib/status';
import { formatTokenCost } from '@/lib/utils';

const CHAT_STATUSES = ['awaiting_followup', 'running_claude', 'pushing', 'creating_preview'];

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
  const showChat = task && CHAT_STATUSES.includes(task.status);
  const canReopen = task && !isViewer && (task.status === 'completed' || task.status === 'failed');
  const canCreatePR = task && !isViewer && task.branch_name && !task.pr_url && (task.status === 'completed' || task.status === 'awaiting_followup');

  return (
    <div>
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="sm" onClick={() => navigate('/tasks')}>
          <ChevronLeft className="size-4" />
          Tasks
        </Button>
        <h1 className="text-2xl font-bold">{task?.name || <span className="font-mono">{id?.slice(0, 8)}</span>}</h1>
        {task && (() => {
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

      {task && (
        <Card className="mb-6">
          <CardContent className="py-4">
            {task.parent_task_id && (
              <>
                <div className="mb-3">
                  <span className="text-muted-foreground text-sm">Parent Task</span>
                  <p className="mt-1">
                    <Link
                      to={`/tasks/${task.parent_task_id}`}
                      className="font-mono text-sm text-blue-400 hover:text-blue-300"
                    >
                      {task.parent_task_id.slice(0, 8)}
                    </Link>
                  </p>
                </div>
                <Separator className="mb-3" />
              </>
            )}
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground">Repo</span>
                <p>{task.repo}</p>
              </div>
              <div>
                <span className="text-muted-foreground">Base</span>
                <p>{task.base_branch}</p>
              </div>
              {task.branch_name && (
                <div>
                  <span className="text-muted-foreground">Branch</span>
                  <p className="font-mono">{task.branch_name}</p>
                </div>
              )}
              {task.preview_url && (
                <div>
                  <span className="text-muted-foreground">Preview</span>
                  <p>
                    <a
                      href={task.preview_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-blue-400 hover:text-blue-300"
                    >
                      {task.preview_slug}
                    </a>
                  </p>
                </div>
              )}
              {task.created_by && (
                <div>
                  <span className="text-muted-foreground">Created by</span>
                  <p className="flex items-center gap-1.5 truncate">
                    <img
                      src={`https://github.com/${task.created_by}.png?size=20`}
                      className="size-4 rounded-full"
                      loading="lazy"
                      alt=""
                    />
                    {task.created_by}
                  </p>
                </div>
              )}
              {(task.total_input_tokens || task.total_output_tokens) ? (
                <div>
                  <span className="text-muted-foreground">Token Usage</span>
                  <p>
                    {(task.total_input_tokens ?? 0).toLocaleString()} in / {(task.total_output_tokens ?? 0).toLocaleString()} out
                  </p>
                  <p className="text-muted-foreground">
                    {formatTokenCost(task.total_input_tokens ?? 0, task.total_output_tokens ?? 0)}
                  </p>
                </div>
              ) : null}
            </div>
            <Separator className="my-3" />
            <div>
              <span className="text-muted-foreground text-sm">Prompt</span>
              <p className="mt-1 text-sm whitespace-pre-wrap">{task.prompt}</p>
              {parseImageUrls(task.image_url).length > 0 && (
                <div className="mt-2 flex flex-wrap gap-2">
                  {parseImageUrls(task.image_url).map((url, i) => (
                    <a key={i} href={url} target="_blank" rel="noopener noreferrer">
                      <img
                        src={url}
                        alt={`Task reference image ${i + 1}`}
                        className="max-w-full rounded-md border border-border hover:opacity-90 transition-opacity"
                        style={{ maxHeight: '300px', objectFit: 'contain' }}
                      />
                    </a>
                  ))}
                </div>
              )}
            </div>
            {task.error_message && (
              <>
                <Separator className="my-3" />
                <div>
                  <span className="text-destructive text-sm">Error</span>
                  <p className="mt-1 text-sm text-destructive">{task.error_message}</p>
                </div>
              </>
            )}
            {task.screenshot_url && (
              <>
                <Separator className="my-3" />
                <div>
                  <span className="text-muted-foreground text-sm">Preview Screenshot</span>
                  <div className="mt-2">
                    <a href={task.screenshot_url} target="_blank" rel="noopener noreferrer">
                      <img
                        src={task.screenshot_url}
                        alt="Preview screenshot"
                        className="max-w-full rounded-md border border-border hover:opacity-90 transition-opacity"
                        style={{ maxHeight: '300px', objectFit: 'contain' }}
                      />
                    </a>
                  </div>
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {showChat && me && !isViewer && (
        <TaskChat taskId={id!} currentUserEmail={me.login} previewUrl={task.preview_url ?? undefined} />
      )}

      {subtasks && subtasks.length > 0 && (
        <Card className="mb-6">
          <CardHeader className="py-3">
            <CardTitle className="text-base">Subtasks</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
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
                              {SubIcon && <SubIcon className={sv.spin ? 'animate-spin' : ''} />}
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
          </CardContent>
        </Card>
      )}

      <div className={task?.branch_name ? 'grid grid-cols-2 gap-6' : 'space-y-6'}>
        <div className="space-y-6">
          <Card>
            <CardHeader className="py-3">
              <CardTitle className="text-base">Live Logs</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              <LogViewer taskId={id!} onConnectionChange={onConnectionChange} />
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="py-3">
              <CardTitle className="text-base">Preview Logs</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              <LogViewer previewSlug={`t-${id!.slice(0, 6)}`} />
            </CardContent>
          </Card>
        </div>
        {task?.branch_name && (
          <Card className="self-start sticky top-4">
            <CardHeader className="py-3">
              <CardTitle className="text-base">Code Diff</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              {diffData?.diff
                ? <DiffViewer diff={diffData.diff} />
                : <p className="p-4 text-sm text-muted-foreground">No diff available yet.</p>}
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
}
