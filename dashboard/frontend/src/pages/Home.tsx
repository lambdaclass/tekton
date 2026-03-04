import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Container, BrainCircuit, ArrowRight } from 'lucide-react';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
    queryKey: ['tasks', { per_page: 8, page: 1 }],
    queryFn: () => listTasks({ per_page: 8, page: 1 }),
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
    <div className="max-w-3xl">
      {/* Header */}
      <div className="flex items-baseline justify-between mb-10">
        <div>
          <h1 className="text-xl font-medium tracking-tight">
            {me?.name || me?.login || 'Dashboard'}
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            {runningTasks.length > 0
              ? `${runningTasks.length} task${runningTasks.length > 1 ? 's' : ''} running`
              : 'No tasks running'}
          </p>
        </div>
        {me?.role !== 'viewer' && (
          <Button asChild size="sm">
            <Link to="/tasks">New task</Link>
          </Button>
        )}
      </div>

      {/* Running tasks */}
      {runningTasks.length > 0 && (
        <section className="mb-10">
          <h2 className="text-xs font-medium uppercase tracking-widest text-muted-foreground mb-4">Active</h2>
          <div className="divide-y divide-border border border-border rounded-md">
            {runningTasks.map((t) => {
              const sv = statusVariant(t.status);
              const StatusIcon = sv.icon;
              return (
                <Link key={t.id} to={`/tasks/${t.id}`} className="flex items-center gap-4 px-4 py-3 hover:bg-secondary/40 transition-colors">
                  <div className="flex-1 min-w-0">
                    <span className="text-sm font-medium">{t.name || t.id.slice(0, 8)}</span>
                    <div className="flex items-center gap-3 mt-0.5 text-xs text-muted-foreground">
                      <span>{t.repo}</span>
                      <span>{timeAgo(t.created_at)}</span>
                    </div>
                  </div>
                  <Badge variant={sv.variant} className={sv.className}>
                    {StatusIcon && <StatusIcon className={`size-3 ${sv.spin ? 'animate-spin' : ''}`} />}
                    {t.status.replace(/_/g, ' ')}
                  </Badge>
                </Link>
              );
            })}
          </div>
        </section>
      )}

      {/* Recent tasks */}
      <section className="mb-10">
        <div className="flex items-baseline justify-between mb-4">
          <h2 className="text-xs font-medium uppercase tracking-widest text-muted-foreground">Recent</h2>
          <Link to="/tasks" className="text-xs text-muted-foreground hover:text-foreground transition-colors inline-flex items-center gap-1">
            All tasks <ArrowRight className="size-3" />
          </Link>
        </div>
        {recentTasks.length > 0 ? (
          <div className="divide-y divide-border border border-border rounded-md">
            {recentTasks.map((t) => {
              const sv = statusVariant(t.status);
              const StatusIcon = sv.icon;
              return (
                <Link key={t.id} to={`/tasks/${t.id}`} className="flex items-center gap-4 px-4 py-3 hover:bg-secondary/40 transition-colors">
                  <div className="flex-1 min-w-0">
                    <span className="text-sm font-medium">{t.name || t.prompt.slice(0, 60)}</span>
                    <div className="flex items-center gap-3 mt-0.5 text-xs text-muted-foreground tabular-nums">
                      <span>{t.repo}</span>
                      {t.total_cost_usd ? <span>{formatCost(t.total_cost_usd)}</span> : null}
                      <span>{timeAgo(t.created_at)}</span>
                    </div>
                  </div>
                  <Badge variant={sv.variant} className={`shrink-0 ${sv.className ?? ''}`}>
                    {StatusIcon && <StatusIcon className={`size-3 ${sv.spin ? 'animate-spin' : ''}`} />}
                    {t.status.replace(/_/g, ' ')}
                  </Badge>
                </Link>
              );
            })}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground py-8 text-center">
            No tasks yet.
          </p>
        )}
      </section>

      {/* Quick links */}
      <div className="grid grid-cols-2 gap-3">
        <Link to="/previews">
          <Card className="hover:bg-secondary/30 transition-colors">
            <CardHeader className="py-4">
              <div className="flex items-center gap-3">
                <Container className="size-4 text-muted-foreground" />
                <CardTitle className="text-sm font-medium">Previews</CardTitle>
                {previews && (
                  <span className="ml-auto text-xs tabular-nums text-muted-foreground">
                    {previews.length}
                  </span>
                )}
              </div>
              <CardDescription className="text-xs">
                Manage preview containers.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
        <Link to="/tasks">
          <Card className="hover:bg-secondary/30 transition-colors">
            <CardHeader className="py-4">
              <div className="flex items-center gap-3">
                <BrainCircuit className="size-4 text-muted-foreground" />
                <CardTitle className="text-sm font-medium">Tasks</CardTitle>
                {allData && (
                  <span className="ml-auto text-xs tabular-nums text-muted-foreground">
                    {allData.total}
                  </span>
                )}
              </div>
              <CardDescription className="text-xs">
                Submit tasks and monitor progress.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
      </div>
    </div>
  );
}
