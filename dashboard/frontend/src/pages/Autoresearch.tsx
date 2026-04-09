import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { FlaskConical, Plus, Loader2 } from 'lucide-react';
import {
  listAutoresearchRuns,
  createAutoresearchRun,
  listAvailableBenchmarkServers,
  listRepos,
} from '@/lib/api';
import type { AutoresearchRun } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { timeAgo } from '@/lib/utils';

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

function improvementPct(run: AutoresearchRun): string | null {
  if (run.baseline_metric == null || run.best_metric == null || run.baseline_metric === 0) return null;
  const raw = ((run.best_metric - run.baseline_metric) / Math.abs(run.baseline_metric)) * 100;
  const pct = run.optimization_direction === 'lower' ? -raw : raw;
  if (pct === 0) return null;
  return `${pct > 0 ? '+' : ''}${pct.toFixed(1)}%`;
}

export default function Autoresearch() {
  const queryClient = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);

  const { data: runs, isLoading } = useQuery({
    queryKey: ['autoresearch-runs'],
    queryFn: listAutoresearchRuns,
    refetchInterval: 5000,
  });

  return (
    <div>
      <div className="flex items-center justify-between mb-10">
        <h1 className="text-xl font-medium tracking-tight">Autoresearch</h1>
        <Button
          variant={showCreate ? 'outline' : 'default'}
          size="sm"
          onClick={() => setShowCreate(!showCreate)}
        >
          {showCreate ? 'Cancel' : <><Plus className="size-3.5 mr-1" /> New Run</>}
        </Button>
      </div>

      {showCreate && (
        <CreateRunForm
          queryClient={queryClient}
          onClose={() => setShowCreate(false)}
        />
      )}

      {isLoading ? (
        <p className="text-sm text-muted-foreground py-8 text-center">Loading runs...</p>
      ) : !runs?.length ? (
        <p className="text-sm text-muted-foreground py-8 text-center">
          No autoresearch runs yet. Start one to optimize your code.
        </p>
      ) : (
        <div className="space-y-2">
          {runs.map((run) => (
            <Link key={run.id} to={`/autoresearch/${run.id}`}>
              <div className="flex items-center justify-between px-4 py-3 rounded-md border border-border hover:bg-secondary/40 transition-colors">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-3">
                    <FlaskConical className="size-4 text-muted-foreground shrink-0" />
                    <span className="font-mono text-sm font-medium truncate">{run.name || run.id.slice(0, 8)}</span>
                    <Badge variant="outline" className={statusColor(run.status)}>
                      {run.status === 'running' && <Loader2 className="size-3 mr-1 animate-spin" />}
                      {run.status.replace(/_/g, ' ')}
                    </Badge>
                    <Badge variant="outline">{run.repo}</Badge>
                    {(() => {
                      const pct = improvementPct(run);
                      if (!pct) return null;
                      const isPositive = pct.startsWith('+');
                      return (
                        <span className={`text-sm font-semibold tabular-nums ${isPositive ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'}`}>
                          {pct}
                        </span>
                      );
                    })()}
                  </div>
                  <div className="flex items-center gap-4 mt-1 text-xs text-muted-foreground ml-7">
                    <span>{run.total_experiments} experiments ({run.accepted_experiments} accepted)</span>
                    {run.total_cost_usd ? <span>${run.total_cost_usd.toFixed(2)}</span> : null}
                    <span>{timeAgo(run.created_at)}</span>
                  </div>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

function CreateRunForm({
  queryClient,
  onClose,
}: {
  queryClient: ReturnType<typeof useQueryClient>;
  onClose: () => void;
}) {
  const [repo, setRepo] = useState('');
  const [baseBranch, setBaseBranch] = useState('main');
  const [benchmarkCommand, setBenchmarkCommand] = useState('');
  const [metricRegex, setMetricRegex] = useState('');
  const [direction, setDirection] = useState<'lower' | 'higher'>('lower');
  const [targetFiles, setTargetFiles] = useState('');
  const [frozenFiles, setFrozenFiles] = useState('');
  const [maxExperiments, setMaxExperiments] = useState('10');
  const [timeBudget, setTimeBudget] = useState('');
  const [serverId, setServerId] = useState<string>('');

  const { data: servers } = useQuery({
    queryKey: ['available-benchmark-servers'],
    queryFn: listAvailableBenchmarkServers,
  });

  const { data: repos } = useQuery({
    queryKey: ['repos'],
    queryFn: listRepos,
  });

  const createMutation = useMutation({
    mutationFn: createAutoresearchRun,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['autoresearch-runs'] });
      onClose();
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      repo,
      base_branch: baseBranch || undefined,
      benchmark_command: benchmarkCommand,
      metric_regex: metricRegex,
      optimization_direction: direction,
      target_files: targetFiles || undefined,
      frozen_files: frozenFiles || undefined,
      max_experiments: maxExperiments ? parseInt(maxExperiments) : undefined,
      time_budget_minutes: timeBudget ? parseInt(timeBudget) : undefined,
      benchmark_server_id: serverId ? parseInt(serverId) : undefined,
    });
  };

  return (
    <Card className="mb-8">
      <CardHeader>
        <CardTitle>New Autoresearch Run</CardTitle>
      </CardHeader>
      <CardContent>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="ar-repo">Repository</Label>
              <Input
                id="ar-repo"
                value={repo}
                onChange={(e) => setRepo(e.target.value)}
                placeholder="owner/repo"
                required
                list="ar-repos-list"
              />
              {repos && (
                <datalist id="ar-repos-list">
                  {repos.map((r) => <option key={r} value={r} />)}
                </datalist>
              )}
            </div>
            <div className="space-y-2">
              <Label htmlFor="ar-branch">Base Branch</Label>
              <Input
                id="ar-branch"
                value={baseBranch}
                onChange={(e) => setBaseBranch(e.target.value)}
                placeholder="main"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="ar-benchmark">Benchmark Command</Label>
              <Input
                id="ar-benchmark"
                value={benchmarkCommand}
                onChange={(e) => setBenchmarkCommand(e.target.value)}
                placeholder="python benchmark.py"
                required
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="ar-regex">Metric Regex (one capture group)</Label>
              <Input
                id="ar-regex"
                value={metricRegex}
                onChange={(e) => setMetricRegex(e.target.value)}
                placeholder="Score: (\d+\.?\d*)"
                required
              />
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label>Optimization Direction</Label>
              <div className="flex gap-4 pt-1">
                <label className="flex items-center gap-1.5 text-sm cursor-pointer">
                  <input type="radio" checked={direction === 'lower'} onChange={() => setDirection('lower')} />
                  Lower is better
                </label>
                <label className="flex items-center gap-1.5 text-sm cursor-pointer">
                  <input type="radio" checked={direction === 'higher'} onChange={() => setDirection('higher')} />
                  Higher is better
                </label>
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="ar-max-exp">Max Experiments</Label>
              <Input
                id="ar-max-exp"
                type="number"
                value={maxExperiments}
                onChange={(e) => setMaxExperiments(e.target.value)}
                placeholder="10"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="ar-time">Time Budget (minutes)</Label>
              <Input
                id="ar-time"
                type="number"
                value={timeBudget}
                onChange={(e) => setTimeBudget(e.target.value)}
                placeholder="60"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="ar-target">Target Files (Claude will focus on these)</Label>
              <Input
                id="ar-target"
                value={targetFiles}
                onChange={(e) => setTargetFiles(e.target.value)}
                placeholder="src/engine.py, src/optimizer.py"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="ar-frozen">Frozen Files (Claude cannot modify)</Label>
              <Input
                id="ar-frozen"
                value={frozenFiles}
                onChange={(e) => setFrozenFiles(e.target.value)}
                placeholder="benchmark.py, tests/"
              />
            </div>
          </div>

          {servers && servers.length > 0 && (
            <div className="space-y-2">
              <Label htmlFor="ar-server">Benchmark Server (optional — leave empty for local)</Label>
              <select
                id="ar-server"
                value={serverId}
                onChange={(e) => setServerId(e.target.value)}
                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
              >
                <option value="">Local (agent container)</option>
                {servers.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} — {s.hostname} {s.hardware_description ? `(${s.hardware_description})` : ''}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className="flex gap-2">
            <Button type="submit" disabled={createMutation.isPending}>
              {createMutation.isPending && <Loader2 className="size-4 animate-spin mr-2" />}
              {createMutation.isPending ? 'Starting...' : 'Start Run'}
            </Button>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
          </div>
          {createMutation.isError && (
            <p className="text-destructive text-sm">{(createMutation.error as Error).message}</p>
          )}
        </form>
      </CardContent>
    </Card>
  );
}
