import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  listUsers,
  setUserRole,
  getUserRepos,
  setUserRepos,
  listSecrets,
  createSecret,
  deleteSecret,
  listPolicies,
  createPolicy,
  deletePolicy,
  listOrgPolicies,
  createOrgPolicy,
  deleteOrgPolicy,
  listPresets,
  getMe,
  getMetricsSummary,
  getTasksOverTime,
  getTopUsers,
  getTopRepos,
} from '@/lib/api';
import type { PolicyPreset, TasksOverTimeRow } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
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
import { Users, KeyRound, Shield, Building, Trash2, Plus, X, Settings, BarChart3, Activity, FolderGit2, DollarSign } from 'lucide-react';
import { Navigate } from 'react-router-dom';

const ROLES = ['admin', 'member', 'viewer'] as const;

export default function Admin() {
  const queryClient = useQueryClient();
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });

  if (me && me.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return (
    <div className="space-y-8">
      <h1 className="text-2xl font-bold">Admin</h1>
      <UsageMetricsSection />
      <UsersSection queryClient={queryClient} />
      <SecretsSection queryClient={queryClient} />
      <PoliciesSection queryClient={queryClient} />
      <OrgPoliciesSection queryClient={queryClient} />
    </div>
  );
}

const METRICS_PERIOD_OPTIONS = [
  { value: '7', label: 'Last 7 days' },
  { value: '30', label: 'Last 30 days' },
  { value: '90', label: 'Last 90 days' },
] as const;

function UsageMetricsSection() {
  const [days, setDays] = useState<number>(30);

  const { data: summary, isLoading: summaryLoading } = useQuery({
    queryKey: ['metrics-summary', days],
    queryFn: () => getMetricsSummary(days),
  });
  const { data: overTime, isLoading: overTimeLoading } = useQuery({
    queryKey: ['metrics-tasks-over-time', days],
    queryFn: () => getTasksOverTime(days),
  });
  const { data: topUsers, isLoading: usersLoading } = useQuery({
    queryKey: ['metrics-top-users', days],
    queryFn: () => getTopUsers(days),
  });
  const { data: topRepos, isLoading: reposLoading } = useQuery({
    queryKey: ['metrics-top-repos', days],
    queryFn: () => getTopRepos(days),
  });

  const fmtCost = (v: number) => (v >= 1 ? `$${v.toFixed(2)}` : `$${v.toFixed(4)}`);

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="size-5" />
            Usage Metrics
          </CardTitle>
          <Select value={String(days)} onValueChange={(v) => setDays(Number(v))}>
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {METRICS_PERIOD_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={o.value}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* Summary cards */}
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          <StatCard
            icon={<Users className="size-4" />}
            label="Active Users"
            value={summaryLoading ? '...' : String(summary?.active_users ?? 0)}
            sub={summary ? `of ${summary.total_users} total` : undefined}
          />
          <StatCard
            icon={<Activity className="size-4" />}
            label="Tasks Created"
            value={summaryLoading ? '...' : String(summary?.total_tasks ?? 0)}
            sub={summary ? `${summary.completed_tasks} completed · ${summary.failed_tasks} failed` : undefined}
          />
          <StatCard
            icon={<DollarSign className="size-4" />}
            label="Total Cost"
            value={summaryLoading ? '...' : summary ? fmtCost(summary.total_cost_usd) : '$0'}
            sub={summary ? `${fmtCost(summary.avg_cost_per_task)} avg/task` : undefined}
          />
          <StatCard
            icon={<FolderGit2 className="size-4" />}
            label="Tokens"
            value={summaryLoading ? '...' : summary ? formatTokens(summary.total_input_tokens + summary.total_output_tokens) : '0'}
            sub={summary ? `${formatTokens(summary.total_input_tokens)} in / ${formatTokens(summary.total_output_tokens)} out` : undefined}
          />
        </div>

        {/* Tasks-over-time chart */}
        <div>
          <h3 className="text-sm font-medium mb-2 text-muted-foreground">Tasks per Day</h3>
          {overTimeLoading ? (
            <p className="text-muted-foreground text-sm">Loading chart...</p>
          ) : !overTime?.length ? (
            <p className="text-muted-foreground text-sm">No task activity in this period.</p>
          ) : (
            <TasksOverTimeChart data={overTime} />
          )}
        </div>

        {/* Top users / repos — side by side */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <h3 className="text-sm font-medium mb-2 text-muted-foreground">Top Users</h3>
            {usersLoading ? (
              <p className="text-muted-foreground text-sm">Loading...</p>
            ) : !topUsers?.length ? (
              <p className="text-muted-foreground text-sm">No data for this period.</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-xs uppercase text-muted-foreground">
                    <th className="pb-2">User</th>
                    <th className="pb-2 text-right">Tasks</th>
                    <th className="pb-2 text-right">Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {topUsers.map((u) => (
                    <tr key={u.login} className="border-b border-border/50 hover:bg-secondary/40">
                      <td className="py-2 font-mono">{u.login}</td>
                      <td className="py-2 text-right tabular-nums">{u.task_count}</td>
                      <td className="py-2 text-right tabular-nums">{fmtCost(u.cost_usd)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
          <div>
            <h3 className="text-sm font-medium mb-2 text-muted-foreground">Top Repos</h3>
            {reposLoading ? (
              <p className="text-muted-foreground text-sm">Loading...</p>
            ) : !topRepos?.length ? (
              <p className="text-muted-foreground text-sm">No data for this period.</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-xs uppercase text-muted-foreground">
                    <th className="pb-2">Repo</th>
                    <th className="pb-2 text-right">Tasks</th>
                    <th className="pb-2 text-right">Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {topRepos.map((r) => (
                    <tr key={r.repo} className="border-b border-border/50 hover:bg-secondary/40">
                      <td className="py-2 font-mono truncate max-w-xs">{r.repo}</td>
                      <td className="py-2 text-right tabular-nums">{r.task_count}</td>
                      <td className="py-2 text-right tabular-nums">{fmtCost(r.cost_usd)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function StatCard({ icon, label, value, sub }: { icon: React.ReactNode; label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="flex items-center gap-2 text-xs text-muted-foreground mb-1">
        {icon}
        {label}
      </div>
      <p className="text-2xl font-bold tabular-nums">{value}</p>
      {sub && <p className="text-xs text-muted-foreground mt-0.5 tabular-nums">{sub}</p>}
    </div>
  );
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function TasksOverTimeChart({ data }: { data: TasksOverTimeRow[] }) {
  const width = 800;
  const height = 180;
  const padLeft = 40;
  const padRight = 20;
  const padTop = 10;
  const padBottom = 28;
  const chartW = width - padLeft - padRight;
  const chartH = height - padTop - padBottom;

  const rawMax = Math.max(...data.map((d) => d.total), 1);

  function niceMax(v: number): number {
    if (v <= 0) return 1;
    const mag = Math.pow(10, Math.floor(Math.log10(v)));
    const norm = v / mag;
    if (norm <= 1) return mag;
    if (norm <= 2) return 2 * mag;
    if (norm <= 5) return 5 * mag;
    return 10 * mag;
  }
  const maxVal = niceMax(rawMax);

  const barW = (chartW / data.length) * 0.7;
  const barGap = (chartW / data.length) * 0.3;

  const yTicks = [0, maxVal / 2, maxVal];

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-44">
      {/* Grid */}
      {yTicks.map((v) => {
        const y = padTop + chartH - (v / maxVal) * chartH;
        return (
          <g key={v}>
            <line x1={padLeft} y1={y} x2={width - padRight} y2={y} stroke="currentColor" strokeOpacity={0.15} />
            <text x={padLeft - 6} y={y + 4} textAnchor="end" className="fill-muted-foreground" style={{ fontSize: 10 }}>
              {Math.round(v)}
            </text>
          </g>
        );
      })}
      {/* Bars — completed (green) + failed (red) stacked */}
      {data.map((d, i) => {
        const x = padLeft + i * (chartW / data.length) + barGap / 2;
        const failedH = (d.failed / maxVal) * chartH;
        const completedH = (d.completed / maxVal) * chartH;
        const otherH = ((d.total - d.completed - d.failed) / maxVal) * chartH;
        const yBase = padTop + chartH;
        const date = new Date(d.day);
        const label = `${date.getMonth() + 1}/${date.getDate()}`;
        return (
          <g key={d.day}>
            <title>{`${label}: ${d.total} tasks (${d.completed} ✓, ${d.failed} ✗)`}</title>
            {/* Other (pending / in progress) */}
            {otherH > 0 && (
              <rect x={x} y={yBase - failedH - completedH - otherH} width={barW} height={otherH} className="fill-muted-foreground/40" />
            )}
            {/* Completed */}
            {completedH > 0 && (
              <rect x={x} y={yBase - failedH - completedH} width={barW} height={completedH} className="fill-emerald-500/70" />
            )}
            {/* Failed */}
            {failedH > 0 && (
              <rect x={x} y={yBase - failedH} width={barW} height={failedH} className="fill-red-500/70" />
            )}
            {data.length <= 31 && (
              <text x={x + barW / 2} y={yBase + 14} textAnchor="middle" className="fill-muted-foreground" style={{ fontSize: 9 }}>
                {label}
              </text>
            )}
          </g>
        );
      })}
      {/* Legend */}
      <g transform={`translate(${padLeft}, ${padTop - 4})`}>
        <rect x={0} y={-8} width={10} height={8} className="fill-emerald-500/70" />
        <text x={14} y={-1} className="fill-muted-foreground" style={{ fontSize: 10 }}>Completed</text>
        <rect x={80} y={-8} width={10} height={8} className="fill-red-500/70" />
        <text x={94} y={-1} className="fill-muted-foreground" style={{ fontSize: 10 }}>Failed</text>
        <rect x={140} y={-8} width={10} height={8} className="fill-muted-foreground/40" />
        <text x={154} y={-1} className="fill-muted-foreground" style={{ fontSize: 10 }}>In progress</text>
      </g>
    </svg>
  );
}

function UsersSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const { data: users, isLoading } = useQuery({
    queryKey: ['admin-users'],
    queryFn: listUsers,
  });

  const [repoDialogUser, setRepoDialogUser] = useState<string | null>(null);

  const roleMutation = useMutation({
    mutationFn: ({ login, role }: { login: string; role: string }) => setUserRole(login, role),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-users'] });
    },
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Users className="size-5" />
          Users
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading users...</p>
        ) : !users?.length ? (
          <p className="text-muted-foreground text-sm">No users found.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Login</th>
                  <th className="pb-2 pr-4 font-medium">Name</th>
                  <th className="pb-2 pr-4 font-medium">Role</th>
                  <th className="pb-2 font-medium">Repos</th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.login} className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
                    <td className="py-2 pr-4 font-mono">{u.login}</td>
                    <td className="py-2 pr-4">{u.name || '-'}</td>
                    <td className="py-2 pr-4">
                      <select
                        value={u.role}
                        onChange={(e) => roleMutation.mutate({ login: u.login, role: e.target.value })}
                        className="h-8 rounded-md border border-input bg-background px-2 text-sm"
                        disabled={roleMutation.isPending}
                      >
                        {ROLES.map((r) => (
                          <option key={r} value={r}>{r}</option>
                        ))}
                      </select>
                    </td>
                    <td className="py-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setRepoDialogUser(u.login)}
                      >
                        <Settings className="size-3 mr-1" />
                        Manage
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {repoDialogUser && (
          <RepoPermissionsDialog
            login={repoDialogUser}
            onClose={() => setRepoDialogUser(null)}
          />
        )}
      </CardContent>
    </Card>
  );
}

function RepoPermissionsDialog({ login, onClose }: { login: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [newRepo, setNewRepo] = useState('');

  const { data: repos, isLoading } = useQuery({
    queryKey: ['admin-user-repos', login],
    queryFn: () => getUserRepos(login),
  });

  const mutation = useMutation({
    mutationFn: (updatedRepos: string[]) => setUserRepos(login, updatedRepos),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-user-repos', login] });
    },
  });

  const handleAdd = () => {
    const trimmed = newRepo.trim();
    if (!trimmed || repos?.includes(trimmed)) return;
    mutation.mutate([...(repos ?? []), trimmed]);
    setNewRepo('');
  };

  const handleRemove = (repo: string) => {
    mutation.mutate((repos ?? []).filter((r) => r !== repo));
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Repo permissions for {login}</DialogTitle>
          <DialogDescription>
            Manage which repositories this user can access.
          </DialogDescription>
        </DialogHeader>

        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading...</p>
        ) : (
          <div className="space-y-3">
            {repos && repos.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {repos.map((r) => (
                  <Badge key={r} variant="secondary" className="gap-1 pr-1">
                    {r}
                    <button
                      onClick={() => handleRemove(r)}
                      className="ml-1 rounded-full hover:bg-muted p-0.5"
                    >
                      <X className="size-3" />
                    </button>
                  </Badge>
                ))}
              </div>
            ) : (
              <p className="text-muted-foreground text-sm">No repos assigned.</p>
            )}

            <div className="flex gap-2">
              <Input
                placeholder="owner/repo"
                value={newRepo}
                onChange={(e) => setNewRepo(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    handleAdd();
                  }
                }}
              />
              <Button size="sm" onClick={handleAdd} disabled={mutation.isPending || !newRepo.trim()}>
                <Plus className="size-4" />
              </Button>
            </div>
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>Close</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SecretsSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const [showAdd, setShowAdd] = useState(false);
  const [deleteId, setDeleteId] = useState<number | null>(null);
  const [newSecret, setNewSecret] = useState({ repo: '', name: '', value: '' });

  const { data: secrets, isLoading } = useQuery({
    queryKey: ['admin-secrets'],
    queryFn: () => listSecrets(),
  });

  const createMutation = useMutation({
    mutationFn: (data: { repo: string; name: string; value: string }) => createSecret(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-secrets'] });
      setShowAdd(false);
      setNewSecret({ repo: '', name: '', value: '' });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: number) => deleteSecret(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-secrets'] });
      setDeleteId(null);
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate(newSecret);
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <KeyRound className="size-5" />
            Secrets
          </CardTitle>
          <Button size="sm" onClick={() => setShowAdd(true)}>
            <Plus className="size-4 mr-1" />
            Add Secret
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading secrets...</p>
        ) : !secrets?.length ? (
          <p className="text-muted-foreground text-sm">No secrets configured.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Name</th>
                  <th className="pb-2 pr-4 font-medium">Repo</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 pr-4 font-medium">Created At</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {secrets.map((s) => (
                  <tr key={s.id} className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
                    <td className="py-2 pr-4 font-mono">{s.name}</td>
                    <td className="py-2 pr-4">{s.repo}</td>
                    <td className="py-2 pr-4">{s.created_by ?? '-'}</td>
                    <td className="py-2 pr-4">
                      {new Date(s.created_at).toLocaleDateString()}
                    </td>
                    <td className="py-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setDeleteId(s.id)}
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

        {/* Add Secret Dialog */}
        <Dialog open={showAdd} onOpenChange={setShowAdd}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Add Secret</DialogTitle>
              <DialogDescription>
                Create a new secret for a repository. The value will be stored encrypted and cannot be viewed after creation.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={handleCreate} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="secret-repo">Repository</Label>
                <Input
                  id="secret-repo"
                  placeholder="owner/repo"
                  value={newSecret.repo}
                  onChange={(e) => setNewSecret((s) => ({ ...s, repo: e.target.value }))}
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="secret-name">Secret Name</Label>
                <Input
                  id="secret-name"
                  placeholder="MY_SECRET_KEY"
                  value={newSecret.name}
                  onChange={(e) => setNewSecret((s) => ({ ...s, name: e.target.value }))}
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="secret-value">Value</Label>
                <Input
                  id="secret-value"
                  type="password"
                  placeholder="Secret value"
                  value={newSecret.value}
                  onChange={(e) => setNewSecret((s) => ({ ...s, value: e.target.value }))}
                  required
                />
              </div>
              <DialogFooter>
                <Button type="button" variant="outline" onClick={() => setShowAdd(false)}>
                  Cancel
                </Button>
                <Button type="submit" disabled={createMutation.isPending}>
                  {createMutation.isPending ? 'Creating...' : 'Create Secret'}
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
              <DialogTitle>Delete Secret</DialogTitle>
              <DialogDescription>
                Are you sure you want to delete this secret? This action cannot be undone.
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

type ToolMode = 'none' | 'deny' | 'allow';
type NetworkMode = 'none' | 'allowlist' | 'denylist';

interface PolicyFormState {
  repo: string;
  protected_branches: string[];
  max_cost_usd: string;
  require_approval_above_usd: string;
  tool_mode: ToolMode;
  tool_list: string[];
  network_mode: NetworkMode;
  network_domains: string[];
}

const INITIAL_POLICY_FORM: PolicyFormState = {
  repo: '',
  protected_branches: ['main', 'master'],
  max_cost_usd: '',
  require_approval_above_usd: '',
  tool_mode: 'none',
  tool_list: [],
  network_mode: 'none',
  network_domains: [],
};

function TagListInput({
  items,
  onAdd,
  onRemove,
  placeholder,
  label,
}: {
  items: string[];
  onAdd: (item: string) => void;
  onRemove: (item: string) => void;
  placeholder: string;
  label?: string;
}) {
  const [input, setInput] = useState('');

  const handleAdd = () => {
    const trimmed = input.trim();
    if (!trimmed || items.includes(trimmed)) return;
    onAdd(trimmed);
    setInput('');
  };

  return (
    <div className="space-y-2">
      {label && <Label>{label}</Label>}
      {items.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {items.map((item) => (
            <Badge key={item} variant="secondary" className="gap-1 pr-1">
              {item}
              <button
                type="button"
                onClick={() => onRemove(item)}
                className="ml-1 rounded-full hover:bg-muted p-0.5"
              >
                <X className="size-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}
      <div className="flex gap-2">
        <Input
          placeholder={placeholder}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              handleAdd();
            }
          }}
        />
        <Button type="button" size="sm" onClick={handleAdd} disabled={!input.trim()}>
          <Plus className="size-4" />
        </Button>
      </div>
    </div>
  );
}

function applyPresetToForm(preset: PolicyPreset): Omit<PolicyFormState, 'repo'> {
  let tool_mode: ToolMode = 'none';
  let tool_list: string[] = [];
  if (preset.allowed_tools?.deny?.length) {
    tool_mode = 'deny';
    tool_list = preset.allowed_tools.deny;
  } else if (preset.allowed_tools?.allow?.length) {
    tool_mode = 'allow';
    tool_list = preset.allowed_tools.allow;
  }

  let network_mode: NetworkMode = 'none';
  let network_domains: string[] = [];
  const net = preset.network_egress;
  if (net) {
    if (net.denylist?.length) {
      network_mode = 'denylist';
      network_domains = net.denylist;
    } else if ((net.allowlist?.length) || (net.allow?.length)) {
      network_mode = 'allowlist';
      network_domains = net.allowlist ?? net.allow ?? [];
    }
  }

  return {
    protected_branches: preset.protected_branches,
    max_cost_usd: preset.max_cost_usd != null ? String(preset.max_cost_usd) : '',
    require_approval_above_usd: preset.require_approval_above_usd != null ? String(preset.require_approval_above_usd) : '',
    tool_mode,
    tool_list,
    network_mode,
    network_domains,
  };
}

function PoliciesSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const [showAdd, setShowAdd] = useState(false);
  const [deleteId, setDeleteId] = useState<number | null>(null);
  const [newPolicy, setNewPolicy] = useState<PolicyFormState>({ ...INITIAL_POLICY_FORM });

  const { data: policies, isLoading } = useQuery({
    queryKey: ['admin-policies'],
    queryFn: () => listPolicies(),
  });

  const { data: presets } = useQuery({
    queryKey: ['policy-presets'],
    queryFn: () => listPresets(),
  });

  const createMutation = useMutation({
    mutationFn: (data: Parameters<typeof createPolicy>[0]) => createPolicy(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-policies'] });
      setShowAdd(false);
      setNewPolicy({ ...INITIAL_POLICY_FORM });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: number) => deletePolicy(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-policies'] });
      setDeleteId(null);
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();

    let allowed_tools: { allow?: string[]; deny?: string[] } | undefined;
    if (newPolicy.tool_mode === 'allow' && newPolicy.tool_list.length > 0) {
      allowed_tools = { allow: newPolicy.tool_list };
    } else if (newPolicy.tool_mode === 'deny' && newPolicy.tool_list.length > 0) {
      allowed_tools = { deny: newPolicy.tool_list };
    }

    let network_egress: { allowlist?: string[]; denylist?: string[] } | undefined;
    if (newPolicy.network_mode === 'allowlist' && newPolicy.network_domains.length > 0) {
      network_egress = { allowlist: newPolicy.network_domains };
    } else if (newPolicy.network_mode === 'denylist' && newPolicy.network_domains.length > 0) {
      network_egress = { denylist: newPolicy.network_domains };
    }

    createMutation.mutate({
      repo: newPolicy.repo,
      protected_branches: newPolicy.protected_branches,
      max_cost_usd: newPolicy.max_cost_usd ? Number(newPolicy.max_cost_usd) : null,
      require_approval_above_usd: newPolicy.require_approval_above_usd ? Number(newPolicy.require_approval_above_usd) : null,
      allowed_tools,
      network_egress,
    });
  };

  const formatToolPolicy = (p: { allow?: string[]; deny?: string[] } | null) => {
    if (!p) return null;
    if (p.deny?.length) return { label: 'Denied', items: p.deny, variant: 'destructive' as const };
    if (p.allow?.length) return { label: 'Allow only', items: p.allow, variant: 'default' as const };
    return null;
  };

  const formatNetworkPolicy = (p: { allowlist?: string[]; denylist?: string[] } | null) => {
    if (!p) return null;
    if (p.denylist?.length) return { label: 'Denied', items: p.denylist, variant: 'destructive' as const };
    if (p.allowlist?.length) return { label: 'Allow only', items: p.allowlist, variant: 'default' as const };
    return null;
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Shield className="size-5" />
            Repo Policies
          </CardTitle>
          <Button size="sm" onClick={() => setShowAdd(true)}>
            <Plus className="size-4 mr-1" />
            Add Policy
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading policies...</p>
        ) : !policies?.length ? (
          <p className="text-muted-foreground text-sm">No policies configured.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Repo</th>
                  <th className="pb-2 pr-4 font-medium">Protected Branches</th>
                  <th className="pb-2 pr-4 font-medium">Tools</th>
                  <th className="pb-2 pr-4 font-medium">Network</th>
                  <th className="pb-2 pr-4 font-medium">Max Cost</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {policies.map((p) => {
                  const toolInfo = formatToolPolicy(p.allowed_tools);
                  const netInfo = formatNetworkPolicy(p.network_egress);
                  return (
                    <tr key={p.id} className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
                      <td className="py-2 pr-4 font-mono">{p.repo}</td>
                      <td className="py-2 pr-4">
                        <div className="flex flex-wrap gap-1">
                          {p.protected_branches?.map((b) => (
                            <Badge key={b} variant="secondary">{b}</Badge>
                          ))}
                        </div>
                      </td>
                      <td className="py-2 pr-4">
                        {toolInfo ? (
                          <div className="flex flex-wrap gap-1">
                            <span className="text-xs text-muted-foreground mr-1">{toolInfo.label}:</span>
                            {toolInfo.items.map((t) => (
                              <Badge key={t} variant={toolInfo.variant} className="text-xs">{t}</Badge>
                            ))}
                          </div>
                        ) : (
                          <span className="text-muted-foreground">-</span>
                        )}
                      </td>
                      <td className="py-2 pr-4">
                        {netInfo ? (
                          <div className="flex flex-wrap gap-1">
                            <span className="text-xs text-muted-foreground mr-1">{netInfo.label}:</span>
                            {netInfo.items.map((d) => (
                              <Badge key={d} variant={netInfo.variant} className="text-xs">{d}</Badge>
                            ))}
                          </div>
                        ) : (
                          <span className="text-muted-foreground">-</span>
                        )}
                      </td>
                      <td className="py-2 pr-4">
                        {p.max_cost_usd != null ? `$${p.max_cost_usd}` : '-'}
                      </td>
                      <td className="py-2 pr-4">{p.created_by ?? '-'}</td>
                      <td className="py-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setDeleteId(p.id)}
                          className="text-destructive hover:text-destructive"
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        {/* Add Policy Dialog */}
        <Dialog open={showAdd} onOpenChange={setShowAdd}>
          <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>Add Policy</DialogTitle>
              <DialogDescription>
                Create a new policy for a repository to enforce branch protection, tool restrictions, network controls, and cost limits.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={handleCreate} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="policy-repo">Repository</Label>
                <Input
                  id="policy-repo"
                  placeholder="owner/repo"
                  value={newPolicy.repo}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, repo: e.target.value }))}
                  required
                />
              </div>

              {/* Preset Selector */}
              {presets && presets.length > 0 && (
                <div className="space-y-2">
                  <Label>Start from Preset</Label>
                  <select
                    value=""
                    onChange={(e) => {
                      const preset = presets.find((p) => p.name === e.target.value);
                      if (preset) {
                        setNewPolicy((prev) => ({ ...prev, ...applyPresetToForm(preset) }));
                      }
                    }}
                    className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                  >
                    <option value="">Custom (no preset)</option>
                    {presets.map((p) => (
                      <option key={p.name} value={p.name}>{p.name} - {p.description}</option>
                    ))}
                  </select>
                </div>
              )}

              {/* Protected Branches */}
              <TagListInput
                label="Protected Branches"
                items={newPolicy.protected_branches}
                onAdd={(b) => setNewPolicy((p) => ({ ...p, protected_branches: [...p.protected_branches, b] }))}
                onRemove={(b) => setNewPolicy((p) => ({ ...p, protected_branches: p.protected_branches.filter((x) => x !== b) }))}
                placeholder="branch name"
              />

              {/* Tool Policy */}
              <div className="space-y-2">
                <Label>Tool Restrictions</Label>
                <select
                  value={newPolicy.tool_mode}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, tool_mode: e.target.value as ToolMode, tool_list: [] }))}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                >
                  <option value="none">No tool restrictions</option>
                  <option value="deny">Deny specific tools</option>
                  <option value="allow">Allow only specific tools</option>
                </select>
                {newPolicy.tool_mode !== 'none' && (
                  <TagListInput
                    items={newPolicy.tool_list}
                    onAdd={(t) => setNewPolicy((p) => ({ ...p, tool_list: [...p.tool_list, t] }))}
                    onRemove={(t) => setNewPolicy((p) => ({ ...p, tool_list: p.tool_list.filter((x) => x !== t) }))}
                    placeholder={newPolicy.tool_mode === 'deny' ? 'Tool name to deny (e.g. Bash)' : 'Tool name to allow (e.g. Read)'}
                  />
                )}
              </div>

              {/* Network Egress Policy */}
              <div className="space-y-2">
                <Label>Network Egress</Label>
                <select
                  value={newPolicy.network_mode}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, network_mode: e.target.value as NetworkMode, network_domains: [] }))}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                >
                  <option value="none">No network restrictions</option>
                  <option value="denylist">Deny specific domains</option>
                  <option value="allowlist">Allow only specific domains</option>
                </select>
                {newPolicy.network_mode !== 'none' && (
                  <TagListInput
                    items={newPolicy.network_domains}
                    onAdd={(d) => setNewPolicy((p) => ({ ...p, network_domains: [...p.network_domains, d] }))}
                    onRemove={(d) => setNewPolicy((p) => ({ ...p, network_domains: p.network_domains.filter((x) => x !== d) }))}
                    placeholder={newPolicy.network_mode === 'denylist' ? 'Domain to deny (e.g. evil.com)' : 'Domain to allow (e.g. github.com)'}
                  />
                )}
              </div>

              {/* Cost Limits */}
              <div className="space-y-2">
                <Label htmlFor="policy-max-cost">Max Cost (USD)</Label>
                <Input
                  id="policy-max-cost"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="Optional"
                  value={newPolicy.max_cost_usd}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, max_cost_usd: e.target.value }))}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="policy-approval-above">Require Approval Above (USD)</Label>
                <Input
                  id="policy-approval-above"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="Optional"
                  value={newPolicy.require_approval_above_usd}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, require_approval_above_usd: e.target.value }))}
                />
              </div>
              <DialogFooter>
                <Button type="button" variant="outline" onClick={() => setShowAdd(false)}>
                  Cancel
                </Button>
                <Button type="submit" disabled={createMutation.isPending}>
                  {createMutation.isPending ? 'Creating...' : 'Create Policy'}
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
              <DialogTitle>Delete Policy</DialogTitle>
              <DialogDescription>
                Are you sure you want to delete this policy? This action cannot be undone.
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

interface OrgPolicyFormState {
  org: string;
  protected_branches: string[];
  max_cost_usd: string;
  require_approval_above_usd: string;
  tool_mode: ToolMode;
  tool_list: string[];
  network_mode: NetworkMode;
  network_domains: string[];
}

const INITIAL_ORG_POLICY_FORM: OrgPolicyFormState = {
  org: '',
  protected_branches: ['main', 'master'],
  max_cost_usd: '',
  require_approval_above_usd: '',
  tool_mode: 'none',
  tool_list: [],
  network_mode: 'none',
  network_domains: [],
};

function OrgPoliciesSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const [showAdd, setShowAdd] = useState(false);
  const [deleteId, setDeleteId] = useState<number | null>(null);
  const [newPolicy, setNewPolicy] = useState<OrgPolicyFormState>({ ...INITIAL_ORG_POLICY_FORM });

  const { data: orgPolicies, isLoading } = useQuery({
    queryKey: ['admin-org-policies'],
    queryFn: () => listOrgPolicies(),
  });

  const { data: presets } = useQuery({
    queryKey: ['policy-presets'],
    queryFn: () => listPresets(),
  });

  const createMutation = useMutation({
    mutationFn: (data: Parameters<typeof createOrgPolicy>[0]) => createOrgPolicy(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-org-policies'] });
      setShowAdd(false);
      setNewPolicy({ ...INITIAL_ORG_POLICY_FORM });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: number) => deleteOrgPolicy(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-org-policies'] });
      setDeleteId(null);
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();

    let allowed_tools: { allow?: string[]; deny?: string[] } | undefined;
    if (newPolicy.tool_mode === 'allow' && newPolicy.tool_list.length > 0) {
      allowed_tools = { allow: newPolicy.tool_list };
    } else if (newPolicy.tool_mode === 'deny' && newPolicy.tool_list.length > 0) {
      allowed_tools = { deny: newPolicy.tool_list };
    }

    let network_egress: { allowlist?: string[]; denylist?: string[] } | undefined;
    if (newPolicy.network_mode === 'allowlist' && newPolicy.network_domains.length > 0) {
      network_egress = { allowlist: newPolicy.network_domains };
    } else if (newPolicy.network_mode === 'denylist' && newPolicy.network_domains.length > 0) {
      network_egress = { denylist: newPolicy.network_domains };
    }

    createMutation.mutate({
      org: newPolicy.org,
      protected_branches: newPolicy.protected_branches,
      max_cost_usd: newPolicy.max_cost_usd ? Number(newPolicy.max_cost_usd) : null,
      require_approval_above_usd: newPolicy.require_approval_above_usd ? Number(newPolicy.require_approval_above_usd) : null,
      allowed_tools,
      network_egress,
    });
  };

  const formatToolPolicy = (p: { allow?: string[]; deny?: string[] } | null) => {
    if (!p) return null;
    if (p.deny?.length) return { label: 'Denied', items: p.deny, variant: 'destructive' as const };
    if (p.allow?.length) return { label: 'Allow only', items: p.allow, variant: 'default' as const };
    return null;
  };

  const formatNetworkPolicy = (p: { allowlist?: string[]; denylist?: string[] } | null) => {
    if (!p) return null;
    if (p.denylist?.length) return { label: 'Denied', items: p.denylist, variant: 'destructive' as const };
    if (p.allowlist?.length) return { label: 'Allow only', items: p.allowlist, variant: 'default' as const };
    return null;
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Building className="size-5" />
            Org Policies
          </CardTitle>
          <Button size="sm" onClick={() => setShowAdd(true)}>
            <Plus className="size-4 mr-1" />
            Add Org Policy
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <p className="text-muted-foreground text-xs mb-4">
          Org policies apply as defaults to all repositories under the organization. Repo-level policies override org-level settings.
        </p>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading org policies...</p>
        ) : !orgPolicies?.length ? (
          <p className="text-muted-foreground text-sm">No org policies configured.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Org</th>
                  <th className="pb-2 pr-4 font-medium">Protected Branches</th>
                  <th className="pb-2 pr-4 font-medium">Tools</th>
                  <th className="pb-2 pr-4 font-medium">Network</th>
                  <th className="pb-2 pr-4 font-medium">Max Cost</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {orgPolicies.map((p) => {
                  const toolInfo = formatToolPolicy(p.allowed_tools);
                  const netInfo = formatNetworkPolicy(p.network_egress);
                  return (
                    <tr key={p.id} className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
                      <td className="py-2 pr-4 font-mono">{p.org}</td>
                      <td className="py-2 pr-4">
                        <div className="flex flex-wrap gap-1">
                          {p.protected_branches?.map((b) => (
                            <Badge key={b} variant="secondary">{b}</Badge>
                          ))}
                        </div>
                      </td>
                      <td className="py-2 pr-4">
                        {toolInfo ? (
                          <div className="flex flex-wrap gap-1">
                            <span className="text-xs text-muted-foreground mr-1">{toolInfo.label}:</span>
                            {toolInfo.items.map((t) => (
                              <Badge key={t} variant={toolInfo.variant} className="text-xs">{t}</Badge>
                            ))}
                          </div>
                        ) : (
                          <span className="text-muted-foreground">-</span>
                        )}
                      </td>
                      <td className="py-2 pr-4">
                        {netInfo ? (
                          <div className="flex flex-wrap gap-1">
                            <span className="text-xs text-muted-foreground mr-1">{netInfo.label}:</span>
                            {netInfo.items.map((d) => (
                              <Badge key={d} variant={netInfo.variant} className="text-xs">{d}</Badge>
                            ))}
                          </div>
                        ) : (
                          <span className="text-muted-foreground">-</span>
                        )}
                      </td>
                      <td className="py-2 pr-4">
                        {p.max_cost_usd != null ? `$${p.max_cost_usd}` : '-'}
                      </td>
                      <td className="py-2 pr-4">{p.created_by ?? '-'}</td>
                      <td className="py-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setDeleteId(p.id)}
                          className="text-destructive hover:text-destructive"
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        {/* Add Org Policy Dialog */}
        <Dialog open={showAdd} onOpenChange={setShowAdd}>
          <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>Add Org Policy</DialogTitle>
              <DialogDescription>
                Create an organization-wide policy. These settings apply as defaults to all repos under the org and can be overridden per-repo.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={handleCreate} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="org-policy-org">Organization</Label>
                <Input
                  id="org-policy-org"
                  placeholder="my-org"
                  value={newPolicy.org}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, org: e.target.value }))}
                  required
                />
              </div>

              {/* Preset Selector */}
              {presets && presets.length > 0 && (
                <div className="space-y-2">
                  <Label>Start from Preset</Label>
                  <select
                    value=""
                    onChange={(e) => {
                      const preset = presets.find((pr) => pr.name === e.target.value);
                      if (preset) {
                        setNewPolicy((prev) => ({ ...prev, ...applyPresetToForm(preset) }));
                      }
                    }}
                    className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                  >
                    <option value="">Custom (no preset)</option>
                    {presets.map((pr) => (
                      <option key={pr.name} value={pr.name}>{pr.name} - {pr.description}</option>
                    ))}
                  </select>
                </div>
              )}

              {/* Protected Branches */}
              <TagListInput
                label="Protected Branches"
                items={newPolicy.protected_branches}
                onAdd={(b) => setNewPolicy((p) => ({ ...p, protected_branches: [...p.protected_branches, b] }))}
                onRemove={(b) => setNewPolicy((p) => ({ ...p, protected_branches: p.protected_branches.filter((x) => x !== b) }))}
                placeholder="branch name"
              />

              {/* Tool Policy */}
              <div className="space-y-2">
                <Label>Tool Restrictions</Label>
                <select
                  value={newPolicy.tool_mode}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, tool_mode: e.target.value as ToolMode, tool_list: [] }))}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                >
                  <option value="none">No tool restrictions</option>
                  <option value="deny">Deny specific tools</option>
                  <option value="allow">Allow only specific tools</option>
                </select>
                {newPolicy.tool_mode !== 'none' && (
                  <TagListInput
                    items={newPolicy.tool_list}
                    onAdd={(t) => setNewPolicy((p) => ({ ...p, tool_list: [...p.tool_list, t] }))}
                    onRemove={(t) => setNewPolicy((p) => ({ ...p, tool_list: p.tool_list.filter((x) => x !== t) }))}
                    placeholder={newPolicy.tool_mode === 'deny' ? 'Tool name to deny (e.g. Bash)' : 'Tool name to allow (e.g. Read)'}
                  />
                )}
              </div>

              {/* Network Egress Policy */}
              <div className="space-y-2">
                <Label>Network Egress</Label>
                <select
                  value={newPolicy.network_mode}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, network_mode: e.target.value as NetworkMode, network_domains: [] }))}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                >
                  <option value="none">No network restrictions</option>
                  <option value="denylist">Deny specific domains</option>
                  <option value="allowlist">Allow only specific domains</option>
                </select>
                {newPolicy.network_mode !== 'none' && (
                  <TagListInput
                    items={newPolicy.network_domains}
                    onAdd={(d) => setNewPolicy((p) => ({ ...p, network_domains: [...p.network_domains, d] }))}
                    onRemove={(d) => setNewPolicy((p) => ({ ...p, network_domains: p.network_domains.filter((x) => x !== d) }))}
                    placeholder={newPolicy.network_mode === 'denylist' ? 'Domain to deny (e.g. evil.com)' : 'Domain to allow (e.g. github.com)'}
                  />
                )}
              </div>

              {/* Cost Limits */}
              <div className="space-y-2">
                <Label htmlFor="org-policy-max-cost">Max Cost (USD)</Label>
                <Input
                  id="org-policy-max-cost"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="Optional"
                  value={newPolicy.max_cost_usd}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, max_cost_usd: e.target.value }))}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="org-policy-approval-above">Require Approval Above (USD)</Label>
                <Input
                  id="org-policy-approval-above"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="Optional"
                  value={newPolicy.require_approval_above_usd}
                  onChange={(e) => setNewPolicy((p) => ({ ...p, require_approval_above_usd: e.target.value }))}
                />
              </div>
              <DialogFooter>
                <Button type="button" variant="outline" onClick={() => setShowAdd(false)}>
                  Cancel
                </Button>
                <Button type="submit" disabled={createMutation.isPending}>
                  {createMutation.isPending ? 'Creating...' : 'Create Org Policy'}
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
              <DialogTitle>Delete Org Policy</DialogTitle>
              <DialogDescription>
                Are you sure you want to delete this org policy? This action cannot be undone.
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
