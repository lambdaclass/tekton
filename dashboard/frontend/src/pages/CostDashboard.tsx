import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Navigate } from 'react-router-dom';
import {
  getMe,
  getCostSummary,
  getCostTrends,
  getCostByUser,
  getCostByRepo,
  listBudgets,
  createBudget,
  deleteBudget,
} from '@/lib/api';
import type { CostTrend, CostGroupRow } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { DollarSign, BarChart3, Users, FolderGit2, Wallet, Plus, Trash2 } from 'lucide-react';

const PERIOD_OPTIONS = [
  { value: '7', label: 'Last 7 days' },
  { value: '30', label: 'Last 30 days' },
  { value: '90', label: 'Last 90 days' },
] as const;

export default function CostDashboard() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const [days, setDays] = useState<number>(30);

  if (me && me.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Cost Dashboard</h1>
        <Select value={String(days)} onValueChange={(v) => setDays(Number(v))}>
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PERIOD_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <SummaryCards days={days} />
      <SpendChart days={days} />
      <CostByUserTable days={days} />
      <CostByRepoTable days={days} />
      <BudgetsSection />
    </div>
  );
}

function SummaryCards({ days }: { days: number }) {
  const { data, isLoading } = useQuery({
    queryKey: ['cost-summary', days],
    queryFn: () => getCostSummary(days),
  });

  const cards = [
    {
      title: 'Total Spend',
      value: data ? `$${data.total_cost_usd.toFixed(2)}` : '-',
      icon: DollarSign,
    },
    {
      title: 'Total Tasks',
      value: data ? String(data.total_tasks) : '-',
      icon: BarChart3,
    },
    {
      title: 'Avg Cost / Task',
      value: data ? `$${data.avg_cost_per_task.toFixed(2)}` : '-',
      icon: Wallet,
    },
  ];

  return (
    <div className="grid gap-4 sm:grid-cols-3">
      {cards.map((c) => (
        <Card key={c.title}>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              {c.title}
            </CardTitle>
            <c.icon className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <p className="text-muted-foreground text-sm">Loading...</p>
            ) : (
              <p className="text-2xl font-bold">{c.value}</p>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function SpendChart({ days }: { days: number }) {
  const { data: trends, isLoading } = useQuery({
    queryKey: ['cost-trends', days],
    queryFn: () => getCostTrends(days),
  });

  const maxCost = trends?.length
    ? Math.max(...trends.map((t: CostTrend) => t.estimated_cost_usd), 0.01)
    : 1;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BarChart3 className="size-5" />
          Daily Spend
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading chart...</p>
        ) : !trends?.length ? (
          <p className="text-muted-foreground text-sm">No data for this period.</p>
        ) : (
          <div className="flex items-end gap-px h-48">
            {trends.map((t: CostTrend) => {
              const pct = (t.estimated_cost_usd / maxCost) * 100;
              const date = new Date(t.day);
              const label = `${date.getMonth() + 1}/${date.getDate()}`;
              return (
                <div
                  key={t.day}
                  className="flex-1 flex flex-col items-center justify-end h-full group relative"
                >
                  <div
                    className="w-full bg-primary/80 rounded-t-sm min-h-[2px] transition-all hover:bg-primary"
                    style={{ height: `${Math.max(pct, 1)}%` }}
                    title={`${label}: $${t.estimated_cost_usd.toFixed(2)} (${t.task_count} tasks)`}
                  />
                  {/* Show label every few bars to avoid crowding */}
                  {(trends.length <= 14 || trends.indexOf(t) % Math.ceil(trends.length / 10) === 0) && (
                    <span className="text-[10px] text-muted-foreground mt-1 leading-none">
                      {label}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const mins = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60);
  if (mins < 60) return `${mins}m ${secs}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

function CostByUserTable({ days }: { days: number }) {
  const { data, isLoading } = useQuery({
    queryKey: ['cost-by-user', days],
    queryFn: () => getCostByUser(days),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Users className="size-5" />
          Cost by User
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading...</p>
        ) : !data?.length ? (
          <p className="text-muted-foreground text-sm">No data for this period.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">User</th>
                  <th className="pb-2 pr-4 font-medium text-right">Input Tokens</th>
                  <th className="pb-2 pr-4 font-medium text-right">Output Tokens</th>
                  <th className="pb-2 pr-4 font-medium text-right">Est. Cost</th>
                  <th className="pb-2 font-medium text-right">Compute Time</th>
                </tr>
              </thead>
              <tbody>
                {data.map((row: CostGroupRow) => (
                  <tr key={row.group_key} className="border-b border-border/50">
                    <td className="py-2 pr-4 font-mono">{row.group_key}</td>
                    <td className="py-2 pr-4 text-right">{row.total_input_tokens.toLocaleString()}</td>
                    <td className="py-2 pr-4 text-right">{row.total_output_tokens.toLocaleString()}</td>
                    <td className="py-2 pr-4 text-right font-medium">${row.estimated_cost_usd.toFixed(2)}</td>
                    <td className="py-2 text-right">{formatDuration(row.total_compute_seconds)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function CostByRepoTable({ days }: { days: number }) {
  const { data, isLoading } = useQuery({
    queryKey: ['cost-by-repo', days],
    queryFn: () => getCostByRepo(days),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <FolderGit2 className="size-5" />
          Cost by Repository
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading...</p>
        ) : !data?.length ? (
          <p className="text-muted-foreground text-sm">No data for this period.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Repository</th>
                  <th className="pb-2 pr-4 font-medium text-right">Input Tokens</th>
                  <th className="pb-2 pr-4 font-medium text-right">Output Tokens</th>
                  <th className="pb-2 pr-4 font-medium text-right">Est. Cost</th>
                  <th className="pb-2 font-medium text-right">Compute Time</th>
                </tr>
              </thead>
              <tbody>
                {data.map((row: CostGroupRow) => (
                  <tr key={row.group_key} className="border-b border-border/50">
                    <td className="py-2 pr-4 font-mono">{row.group_key}</td>
                    <td className="py-2 pr-4 text-right">{row.total_input_tokens.toLocaleString()}</td>
                    <td className="py-2 pr-4 text-right">{row.total_output_tokens.toLocaleString()}</td>
                    <td className="py-2 pr-4 text-right font-medium">${row.estimated_cost_usd.toFixed(2)}</td>
                    <td className="py-2 text-right">{formatDuration(row.total_compute_seconds)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function BudgetsSection() {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [deleteId, setDeleteId] = useState<number | null>(null);
  const [newBudget, setNewBudget] = useState({
    scope_type: 'user',
    scope: '',
    monthly_limit_usd: '',
    alert_threshold_pct: '80',
  });

  const { data: budgets, isLoading } = useQuery({
    queryKey: ['budgets'],
    queryFn: listBudgets,
  });

  const createMutation = useMutation({
    mutationFn: (data: Parameters<typeof createBudget>[0]) => createBudget(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['budgets'] });
      setShowAdd(false);
      setNewBudget({ scope_type: 'user', scope: '', monthly_limit_usd: '', alert_threshold_pct: '80' });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: number) => deleteBudget(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['budgets'] });
      setDeleteId(null);
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      scope_type: newBudget.scope_type,
      scope: newBudget.scope,
      monthly_limit_usd: Number(newBudget.monthly_limit_usd),
      alert_threshold_pct: Number(newBudget.alert_threshold_pct),
    });
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Wallet className="size-5" />
            Budgets
          </CardTitle>
          <Button size="sm" onClick={() => setShowAdd(true)}>
            <Plus className="size-4 mr-1" />
            Add Budget
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading budgets...</p>
        ) : !budgets?.length ? (
          <p className="text-muted-foreground text-sm">No budgets configured.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Scope</th>
                  <th className="pb-2 pr-4 font-medium">Value</th>
                  <th className="pb-2 pr-4 font-medium text-right">Monthly Limit</th>
                  <th className="pb-2 pr-4 font-medium text-right">Alert Threshold</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {budgets.map((b) => (
                  <tr key={b.id} className="border-b border-border/50">
                    <td className="py-2 pr-4">
                      <span className="inline-flex items-center rounded-md bg-muted px-2 py-0.5 text-xs font-medium">
                        {b.scope_type}
                      </span>
                    </td>
                    <td className="py-2 pr-4 font-mono">{b.scope}</td>
                    <td className="py-2 pr-4 text-right font-medium">${b.monthly_limit_usd.toFixed(2)}</td>
                    <td className="py-2 pr-4 text-right">{b.alert_threshold_pct}%</td>
                    <td className="py-2 pr-4">{b.created_by ?? '-'}</td>
                    <td className="py-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setDeleteId(b.id)}
                        className="text-destructive hover:text-destructive"
                      >
                        <Trash2 className="size-4" />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Add Budget Dialog */}
        <Dialog open={showAdd} onOpenChange={setShowAdd}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Add Budget</DialogTitle>
              <DialogDescription>
                Set a monthly spending limit. You will be alerted when usage reaches the threshold percentage.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={handleCreate} className="space-y-4">
              <div className="space-y-2">
                <Label>Scope Type</Label>
                <Select
                  value={newBudget.scope_type}
                  onValueChange={(v) => setNewBudget((s) => ({ ...s, scope_type: v }))}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="user">User</SelectItem>
                    <SelectItem value="org">Organization</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label htmlFor="budget-scope-value">
                  {newBudget.scope_type === 'user' ? 'Username' : 'Organization'}
                </Label>
                <Input
                  id="budget-scope-value"
                  placeholder={newBudget.scope_type === 'user' ? 'github-login' : 'org-name'}
                  value={newBudget.scope}
                  onChange={(e) => setNewBudget((s) => ({ ...s, scope: e.target.value }))}
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="budget-limit">Monthly Limit (USD)</Label>
                <Input
                  id="budget-limit"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="100.00"
                  value={newBudget.monthly_limit_usd}
                  onChange={(e) => setNewBudget((s) => ({ ...s, monthly_limit_usd: e.target.value }))}
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="budget-threshold">Alert Threshold (%)</Label>
                <Input
                  id="budget-threshold"
                  type="number"
                  min="1"
                  max="100"
                  placeholder="80"
                  value={newBudget.alert_threshold_pct}
                  onChange={(e) => setNewBudget((s) => ({ ...s, alert_threshold_pct: e.target.value }))}
                  required
                />
              </div>
              <DialogFooter>
                <Button type="button" variant="outline" onClick={() => setShowAdd(false)}>
                  Cancel
                </Button>
                <Button type="submit" disabled={createMutation.isPending}>
                  {createMutation.isPending ? 'Creating...' : 'Create Budget'}
                </Button>
              </DialogFooter>
              {createMutation.isError && (
                <p className="text-destructive text-sm">
                  {(createMutation.error as Error).message}
                </p>
              )}
            </form>
          </DialogContent>
        </Dialog>

        {/* Delete Confirmation Dialog */}
        <Dialog open={deleteId !== null} onOpenChange={(open) => { if (!open) setDeleteId(null); }}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Delete Budget</DialogTitle>
              <DialogDescription>
                Are you sure you want to delete this budget? This action cannot be undone.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={() => setDeleteId(null)}>
                Cancel
              </Button>
              <Button
                variant="destructive"
                onClick={() => deleteId !== null && deleteMutation.mutate(deleteId)}
                disabled={deleteMutation.isPending}
              >
                {deleteMutation.isPending ? 'Deleting...' : 'Delete'}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardContent>
    </Card>
  );
}
