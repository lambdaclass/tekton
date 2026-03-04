import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Container, BrainCircuit, Plus } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { listPreviews, listTasks, getMe } from '@/lib/api';
import { statusVariant } from '@/lib/status';
import { timeAgo, formatCost } from '@/lib/utils';

const ACTIVE_STATUSES = new Set([
  'creating_agent',
  'cloning',
  'running_claude',
  'pushing',
  'creating_preview',
]);

export default function Home() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const { data: previews } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
  });
  const { data: recentData } = useQuery({
    queryKey: ['tasks', { per_page: 5, page: 1 }],
    queryFn: () => listTasks({ per_page: 5, page: 1 }),
    refetchInterval: 5000,
  });
  const { data: allData } = useQuery({
    queryKey: ['tasks', { per_page: 200 }],
    queryFn: () => listTasks({ per_page: 200 }),
    refetchInterval: 5000,
  });

  const recentTasks = recentData?.tasks ?? [];
  const runningTasks = (allData?.tasks ?? []).filter((t) => ACTIVE_STATUSES.has(t.status));

  return (
    <div>
      {/* Welcome section */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold">
            Welcome back{(me?.name || me?.login) ? ', ' : ''}{(me?.name || me?.login) ? (
              <span className="bg-gradient-to-r from-primary to-blue-400 bg-clip-text text-transparent">
                {me?.name || me?.login}
              </span>
            ) : ''}
          </h1>
          <p className="text-muted-foreground mt-1">
            Here is what is happening across your projects.
          </p>
        </div>
        {me?.role !== 'viewer' && (
          <Button asChild className="btn-gradient">
            <Link to="/tasks">
              <Plus className="size-4 mr-1" />
              Create Task
            </Link>
          </Button>
        )}
      </div>

      {/* Running tasks highlight */}
      {runningTasks.length > 0 && (
        <div className="mb-6">
          <h2 className="text-sm font-medium text-muted-foreground mb-3">Currently Running</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {runningTasks.map((t) => {
              const sv = statusVariant(t.status);
              const StatusIcon = sv.icon;
              return (
                <Link key={t.id} to={`/tasks/${t.id}`}>
                  <Card className="gradient-border hover:border-primary/40 transition-colors">
                    <CardContent className="py-3">
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-sm font-medium truncate mr-2">
                          {t.name || t.id.slice(0, 8)}
                        </span>
                        <Badge variant={sv.variant} className={sv.className}>
                          {StatusIcon && <StatusIcon className={sv.spin ? 'animate-spin' : ''} />}
                          {t.status}
                        </Badge>
                      </div>
                      <p className="text-xs text-muted-foreground line-clamp-1">{t.prompt}</p>
                      <div className="flex items-center gap-3 mt-1.5 text-xs text-muted-foreground">
                        <span>{t.repo}</span>
                        <span>{timeAgo(t.created_at)}</span>
                      </div>
                    </CardContent>
                  </Card>
                </Link>
              );
            })}
          </div>
        </div>
      )}

      {/* Recent tasks */}
      <div className="mb-8">
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-medium text-muted-foreground">Recent Tasks</h2>
          <Link to="/tasks" className="text-xs text-primary hover:underline">
            View all
          </Link>
        </div>
        {recentTasks.length > 0 ? (
          <div className="space-y-2">
            {recentTasks.map((t) => {
              const sv = statusVariant(t.status);
              const StatusIcon = sv.icon;
              return (
                <Link key={t.id} to={`/tasks/${t.id}`}>
                  <div className="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-primary/5 transition-colors">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium truncate">
                          {t.name || t.prompt.slice(0, 60)}
                        </span>
                      </div>
                      <div className="flex items-center gap-3 mt-0.5 text-xs text-muted-foreground">
                        <span>{t.repo}</span>
                        {t.total_cost_usd ? <span>{formatCost(t.total_cost_usd)}</span> : null}
                        <span>{timeAgo(t.created_at)}</span>
                      </div>
                    </div>
                    <Badge variant={sv.variant} className={`shrink-0 ${sv.className ?? ''}`}>
                      {StatusIcon && <StatusIcon className={sv.spin ? 'animate-spin' : ''} />}
                      {t.status}
                    </Badge>
                  </div>
                </Link>
              );
            })}
          </div>
        ) : (
          <div className="text-center py-8 text-muted-foreground">
            <BrainCircuit className="size-8 mx-auto mb-2 opacity-50" />
            <p className="text-sm">No tasks yet. Create your first task to get started.</p>
          </div>
        )}
      </div>

      {/* Quick links */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Link to="/previews">
          <Card className="hover:border-muted-foreground/25 transition-colors">
            <CardHeader>
              <div className="flex items-center gap-3">
                <Container className="size-5 text-muted-foreground" />
                <CardTitle>Previews</CardTitle>
                {previews && (
                  <span className="ml-auto text-sm tabular-nums text-muted-foreground">
                    {previews.length}
                  </span>
                )}
              </div>
              <CardDescription>
                Manage preview containers. Create, destroy, and update previews from any branch.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
        <Link to="/tasks">
          <Card className="hover:border-muted-foreground/25 transition-colors">
            <CardHeader>
              <div className="flex items-center gap-3">
                <BrainCircuit className="size-5 text-muted-foreground" />
                <CardTitle>Tasks</CardTitle>
                {allData && (
                  <span className="ml-auto text-sm tabular-nums text-muted-foreground">
                    {allData.total}
                  </span>
                )}
              </div>
              <CardDescription>
                Submit coding tasks to your AI agent. Monitor progress and view live output.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
      </div>
    </div>
  );
}
