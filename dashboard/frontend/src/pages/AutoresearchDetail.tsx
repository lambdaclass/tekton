import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  ChevronLeft,
  FlaskConical,
  GitPullRequest,
  Square,
  Check,
  X,
  Loader2,
  ScrollText,
  BarChart3,
  Settings,
  Clock,
  TrendingUp,
  Zap,
  DollarSign,
  Target,
} from 'lucide-react';
import {
  getAutoresearchRun,
  listAutoresearchExperiments,
  getAutoresearchStats,
  stopAutoresearchRun,
  createAutoresearchPR,
} from '@/lib/api';
import type { AutoresearchExperiment } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import MetricChart from '@/components/MetricChart';
import LogViewer from '@/components/LogViewer';


function statusColor(status: string) {
  switch (status) {
    case 'running': return 'bg-blue-500/10 text-blue-700 dark:text-blue-400 border-blue-500/30';
    case 'completed': return 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 border-emerald-500/30';
    case 'stopped': return 'bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-500/30';
    case 'failed': return 'bg-red-500/10 text-red-700 dark:text-red-400 border-red-500/30';
    case 'setting_up': return 'bg-purple-500/10 text-purple-700 dark:text-purple-400 border-purple-500/30';
    default: return 'bg-secondary text-muted-foreground';
  }
}

export default function AutoresearchDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [expandedExp, setExpandedExp] = useState<number | null>(null);

  const isActive = (status: string) => ['running', 'setting_up', 'pending'].includes(status);

  const { data: run } = useQuery({
    queryKey: ['autoresearch-run', id],
    queryFn: () => getAutoresearchRun(id!),
    enabled: !!id,
    refetchInterval: 3000,
  });

  const { data: experiments } = useQuery({
    queryKey: ['autoresearch-experiments', id],
    queryFn: () => listAutoresearchExperiments(id!),
    enabled: !!id,
    refetchInterval: run && isActive(run.status) ? 5000 : undefined,
  });

  const { data: stats } = useQuery({
    queryKey: ['autoresearch-stats', id],
    queryFn: () => getAutoresearchStats(id!),
    enabled: !!id,
    refetchInterval: run && isActive(run.status) ? 5000 : undefined,
  });

  const stopMutation = useMutation({
    mutationFn: () => stopAutoresearchRun(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['autoresearch-run', id] });
    },
  });

  const prMutation = useMutation({
    mutationFn: () => createAutoresearchPR(id!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['autoresearch-run', id] });
    },
  });

  if (!run) {
    return <p className="text-sm text-muted-foreground py-8 text-center">Loading...</p>;
  }

  const running = isActive(run.status);

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* Top bar */}
      <div className="flex items-center gap-2 px-1 pb-2 shrink-0">
        <Button variant="ghost" size="icon-sm" onClick={() => navigate('/autoresearch')} aria-label="Back">
          <ChevronLeft className="size-4" />
        </Button>
        <FlaskConical className="size-4 text-muted-foreground" />
        <h1 className="text-lg font-semibold truncate max-w-md">
          {run.name || run.id.slice(0, 8)}
        </h1>
        <Badge variant="outline" className={statusColor(run.status)}>
          {running && <Loader2 className="size-3 mr-1 animate-spin" />}
          {run.status.replace(/_/g, ' ')}
        </Badge>
        <Badge variant="outline">{run.repo}</Badge>
        {run.branch_name && (
          <span className="text-xs text-muted-foreground font-mono">({run.branch_name})</span>
        )}

        <div className="ml-auto flex items-center gap-2">
          {running && (
            <Button
              size="sm"
              variant="outline"
              className="text-destructive border-destructive/30 hover:bg-destructive/10"
              onClick={() => stopMutation.mutate()}
              disabled={stopMutation.isPending}
            >
              <Square className="size-3.5 mr-1" />
              {stopMutation.isPending ? 'Stopping...' : 'Stop'}
            </Button>
          )}
          {!running && run.accepted_experiments > 0 && !run.pr_url && (
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
          {run.pr_url && (
            <a href={run.pr_url} target="_blank" rel="noopener noreferrer">
              <Button size="sm" variant="outline">
                <GitPullRequest className="size-3.5 mr-1" />
                PR #{run.pr_number}
              </Button>
            </a>
          )}
        </div>
      </div>

      {/* Stats bar */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-7 gap-3 px-1 pb-3 border-b border-border/50 shrink-0">
        <StatCard
          icon={<TrendingUp className="size-3.5" />}
          label="Improvement"
          value={stats ? `${stats.improvement_pct > 0 ? '+' : ''}${stats.improvement_pct.toFixed(1)}%` : '—'}
          highlight={stats && stats.improvement_pct > 0}
        />
        <StatCard
          icon={<Target className="size-3.5" />}
          label="Best"
          value={run.best_metric != null ? run.best_metric.toFixed(4) : '—'}
        />
        <StatCard
          icon={<Target className="size-3.5" />}
          label="Baseline"
          value={run.baseline_metric != null ? run.baseline_metric.toFixed(4) : '—'}
        />
        <StatCard
          icon={<BarChart3 className="size-3.5" />}
          label="Experiments"
          value={`${run.total_experiments} (${run.accepted_experiments} accepted)`}
        />
        <StatCard
          icon={<Zap className="size-3.5" />}
          label="Rate"
          value={stats ? `${stats.experiments_per_hour.toFixed(1)}/hr` : '—'}
        />
        <StatCard
          icon={<Clock className="size-3.5" />}
          label="Est. Remaining"
          value={stats?.est_remaining_minutes != null ? `${Math.max(0, stats.est_remaining_minutes).toFixed(0)}m` : '—'}
        />
        <StatCard
          icon={<DollarSign className="size-3.5" />}
          label="Cost"
          value={run.total_cost_usd ? `$${run.total_cost_usd.toFixed(2)}` : '$0.00'}
        />
      </div>

      {/* Error banner */}
      {run.error_message && (
        <div className="mx-1 my-2 rounded-md border border-destructive/40 bg-destructive/5 px-4 py-2 text-sm text-destructive shrink-0">
          {run.error_message}
        </div>
      )}

      {/* Tabs */}
      <Tabs defaultValue="experiments" className="flex flex-col flex-1 min-h-0 pt-3">
        <TabsList variant="line" className="shrink-0 border-b border-border pb-0 mb-0">
          <TabsTrigger value="experiments" className="gap-1.5">
            <BarChart3 className="size-3.5" />
            Experiments
          </TabsTrigger>
          <TabsTrigger value="logs" className="gap-1.5">
            <ScrollText className="size-3.5" />
            Logs
          </TabsTrigger>
          <TabsTrigger value="config" className="gap-1.5">
            <Settings className="size-3.5" />
            Config
          </TabsTrigger>
        </TabsList>

        {/* Experiments tab */}
        <TabsContent value="experiments" className="flex-1 flex flex-col min-h-0 rounded-b-lg border border-t-0 border-border bg-card overflow-hidden">
          <div className="flex flex-col lg:flex-row flex-1 min-h-0">
            {/* Chart */}
            <div className="h-48 lg:h-auto lg:flex-1 border-b lg:border-b-0 lg:border-r border-border p-4">
              <MetricChart
                experiments={experiments ?? []}
                baseline={run.baseline_metric}
                best={run.best_metric}
              />
            </div>

            {/* Experiment feed */}
            <div className="flex-1 lg:w-96 lg:flex-none overflow-y-auto">
              {!experiments?.length ? (
                <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
                  {running ? (
                    <><Loader2 className="size-4 mr-2 animate-spin" /> Waiting for first experiment...</>
                  ) : (
                    'No experiments'
                  )}
                </div>
              ) : (
                <div className="divide-y divide-border">
                  {[...experiments].reverse().map((exp) => (
                    <ExperimentRow
                      key={exp.id}
                      exp={exp}
                      expanded={expandedExp === exp.id}
                      onToggle={() => setExpandedExp(expandedExp === exp.id ? null : exp.id)}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>
        </TabsContent>

        {/* Logs tab */}
        <TabsContent value="logs" forceMount className="flex-1 flex flex-col min-h-0 rounded-b-lg border border-t-0 border-border bg-card overflow-hidden data-[state=inactive]:hidden">
          <div className="flex-1 min-h-0">
            <LogViewer autoresearchRunId={id!} />
          </div>
        </TabsContent>

        {/* Config tab */}
        <TabsContent value="config" className="flex-1 overflow-y-auto rounded-b-lg border border-t-0 border-border bg-card p-4">
          <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
            <dt className="text-muted-foreground">Repository</dt>
            <dd>{run.repo}</dd>
            <dt className="text-muted-foreground">Base Branch</dt>
            <dd className="font-mono">{run.base_branch}</dd>
            {run.objective && (
              <>
                <dt className="text-muted-foreground">Objective</dt>
                <dd>{run.objective}</dd>
              </>
            )}
            <dt className="text-muted-foreground">Benchmark Command</dt>
            <dd className="font-mono">{run.benchmark_command}</dd>
            {run.target_files && (
              <>
                <dt className="text-muted-foreground">Target Files</dt>
                <dd className="font-mono">{run.target_files}</dd>
              </>
            )}
            {run.frozen_files && (
              <>
                <dt className="text-muted-foreground">Frozen Files</dt>
                <dd className="font-mono">{run.frozen_files}</dd>
              </>
            )}
            {run.max_experiments && (
              <>
                <dt className="text-muted-foreground">Max Experiments</dt>
                <dd>{run.max_experiments}</dd>
              </>
            )}
            {run.time_budget_minutes && (
              <>
                <dt className="text-muted-foreground">Time Budget</dt>
                <dd>{run.time_budget_minutes} minutes</dd>
              </>
            )}
            <dt className="text-muted-foreground">Run ID</dt>
            <dd className="font-mono text-xs">{run.id}</dd>
            <dt className="text-muted-foreground">Created By</dt>
            <dd>{run.created_by ?? '—'}</dd>
            <dt className="text-muted-foreground">Created</dt>
            <dd>{new Date(run.created_at).toLocaleString()}</dd>
          </dl>
        </TabsContent>
      </Tabs>
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
  highlight,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <div className="rounded-md border border-border bg-card/50 px-3 py-2">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted-foreground mb-0.5">
        {icon}
        {label}
      </div>
      <div className={`text-sm font-semibold tabular-nums ${highlight ? 'text-emerald-600 dark:text-emerald-400' : ''}`}>
        {value}
      </div>
    </div>
  );
}

function ExperimentRow({
  exp,
  expanded,
  onToggle,
}: {
  exp: AutoresearchExperiment;
  expanded: boolean;
  onToggle: () => void;
}) {
  const isRunning = exp.status === 'running' || exp.status === 'benchmarking';

  return (
    <div className="px-4 py-3">
      <button onClick={onToggle} className="w-full text-left">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-muted-foreground w-8">#{exp.experiment_number}</span>
          {isRunning ? (
            <Loader2 className="size-3.5 animate-spin text-blue-500" />
          ) : exp.accepted ? (
            <Check className="size-3.5 text-emerald-500" />
          ) : (
            <X className="size-3.5 text-red-500" />
          )}
          <span className="text-sm font-mono tabular-nums">
            {exp.metric_value != null ? exp.metric_value.toFixed(4) : isRunning ? exp.status : 'N/A'}
          </span>
          {exp.duration_seconds != null && (
            <span className="text-xs text-muted-foreground ml-auto">{exp.duration_seconds}s</span>
          )}
        </div>
        {exp.claude_response && (
          <p className="text-xs text-muted-foreground mt-1 line-clamp-1 ml-8">
            {exp.claude_response.slice(0, 120)}
          </p>
        )}
      </button>
      {expanded && exp.diff && (
        <pre className="mt-2 ml-8 text-[11px] font-mono bg-secondary rounded-md p-2 overflow-x-auto max-h-48 overflow-y-auto whitespace-pre">
          {exp.diff}
        </pre>
      )}
    </div>
  );
}
