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
  getMe,
  getPoolStatus,
  resizePool,
  refillPool,
} from '@/lib/api';
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
import { Users, KeyRound, Shield, Trash2, Plus, X, Settings, Server } from 'lucide-react';
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
      <PoolSection queryClient={queryClient} />
      <UsersSection queryClient={queryClient} />
      <SecretsSection queryClient={queryClient} />
      <PoliciesSection queryClient={queryClient} />
    </div>
  );
}

function PoolSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const { data: pool, isLoading } = useQuery({
    queryKey: ['admin-pool'],
    queryFn: getPoolStatus,
    refetchInterval: 10000,
  });

  const [newTarget, setNewTarget] = useState('');

  const resizeMutation = useMutation({
    mutationFn: (target: number) => resizePool(target),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-pool'] });
      setNewTarget('');
    },
  });

  const refillMutation = useMutation({
    mutationFn: () => refillPool(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-pool'] });
    },
  });

  const statusColor = !pool
    ? 'bg-muted'
    : pool.available === 0
      ? 'bg-red-500'
      : pool.available < pool.target
        ? 'bg-yellow-500'
        : 'bg-green-500';

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Server className="size-5" />
          Agent Pool
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading pool status...</p>
        ) : pool ? (
          <>
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2">
                <span className={`inline-block size-3 rounded-full ${statusColor}`} />
                <span className="text-sm font-medium">
                  {pool.available} / {pool.target} available
                </span>
              </div>
              {pool.available < pool.target && (
                <Badge variant="outline" className="text-yellow-600">
                  {pool.target - pool.available} deficit
                </Badge>
              )}
            </div>

            {pool.containers.length > 0 && (
              <div className="text-sm text-muted-foreground">
                <p className="font-medium text-foreground mb-1">Ready containers:</p>
                <div className="flex flex-wrap gap-2">
                  {pool.containers.map((c) => (
                    <Badge key={c.name} variant="secondary">
                      {c.name} {c.ip ? `(${c.ip})` : ''}
                    </Badge>
                  ))}
                </div>
              </div>
            )}

            <div className="flex items-end gap-2 pt-2">
              <div className="space-y-1">
                <Label htmlFor="pool-target" className="text-xs">Target size</Label>
                <Input
                  id="pool-target"
                  type="number"
                  min={0}
                  max={20}
                  value={newTarget}
                  onChange={(e) => setNewTarget(e.target.value)}
                  placeholder={String(pool.target)}
                  className="w-24"
                />
              </div>
              <Button
                size="sm"
                onClick={() => {
                  const val = parseInt(newTarget, 10);
                  if (!isNaN(val) && val >= 0 && val <= 20) resizeMutation.mutate(val);
                }}
                disabled={resizeMutation.isPending || !newTarget}
              >
                {resizeMutation.isPending ? 'Resizing...' : 'Resize'}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => refillMutation.mutate()}
                disabled={refillMutation.isPending}
              >
                {refillMutation.isPending ? 'Refilling...' : 'Refill Now'}
              </Button>
            </div>
          </>
        ) : (
          <p className="text-muted-foreground text-sm">Could not load pool status.</p>
        )}
      </CardContent>
    </Card>
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
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Login</th>
                  <th className="pb-2 pr-4 font-medium">Name</th>
                  <th className="pb-2 pr-4 font-medium">Role</th>
                  <th className="pb-2 font-medium">Repos</th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.login} className="border-b border-border/50">
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
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Name</th>
                  <th className="pb-2 pr-4 font-medium">Repo</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 pr-4 font-medium">Created At</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {secrets.map((s) => (
                  <tr key={s.id} className="border-b border-border/50">
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

function PoliciesSection({ queryClient }: { queryClient: ReturnType<typeof useQueryClient> }) {
  const [showAdd, setShowAdd] = useState(false);
  const [deleteId, setDeleteId] = useState<number | null>(null);
  const [newPolicy, setNewPolicy] = useState({
    repo: '',
    protected_branches: ['main', 'master'] as string[],
    max_cost_usd: '' as string,
    require_approval_above_usd: '' as string,
  });
  const [branchInput, setBranchInput] = useState('');

  const { data: policies, isLoading } = useQuery({
    queryKey: ['admin-policies'],
    queryFn: () => listPolicies(),
  });

  const createMutation = useMutation({
    mutationFn: (data: Parameters<typeof createPolicy>[0]) => createPolicy(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-policies'] });
      setShowAdd(false);
      setNewPolicy({
        repo: '',
        protected_branches: ['main', 'master'],
        max_cost_usd: '',
        require_approval_above_usd: '',
      });
      setBranchInput('');
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
    createMutation.mutate({
      repo: newPolicy.repo,
      protected_branches: newPolicy.protected_branches,
      max_cost_usd: newPolicy.max_cost_usd ? Number(newPolicy.max_cost_usd) : null,
      require_approval_above_usd: newPolicy.require_approval_above_usd ? Number(newPolicy.require_approval_above_usd) : null,
    });
  };

  const addBranch = () => {
    const trimmed = branchInput.trim();
    if (!trimmed || newPolicy.protected_branches.includes(trimmed)) return;
    setNewPolicy((p) => ({ ...p, protected_branches: [...p.protected_branches, trimmed] }));
    setBranchInput('');
  };

  const removeBranch = (branch: string) => {
    setNewPolicy((p) => ({ ...p, protected_branches: p.protected_branches.filter((b) => b !== branch) }));
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
                <tr className="border-b text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Repo</th>
                  <th className="pb-2 pr-4 font-medium">Protected Branches</th>
                  <th className="pb-2 pr-4 font-medium">Max Cost</th>
                  <th className="pb-2 pr-4 font-medium">Created By</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {policies.map((p) => (
                  <tr key={p.id} className="border-b border-border/50">
                    <td className="py-2 pr-4 font-mono">{p.repo}</td>
                    <td className="py-2 pr-4">
                      <div className="flex flex-wrap gap-1">
                        {p.protected_branches?.map((b) => (
                          <Badge key={b} variant="secondary">{b}</Badge>
                        ))}
                      </div>
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
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Add Policy Dialog */}
        <Dialog open={showAdd} onOpenChange={setShowAdd}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Add Policy</DialogTitle>
              <DialogDescription>
                Create a new policy for a repository to enforce branch protection and cost limits.
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
              <div className="space-y-2">
                <Label>Protected Branches</Label>
                {newPolicy.protected_branches.length > 0 && (
                  <div className="flex flex-wrap gap-2">
                    {newPolicy.protected_branches.map((b) => (
                      <Badge key={b} variant="secondary" className="gap-1 pr-1">
                        {b}
                        <button
                          type="button"
                          onClick={() => removeBranch(b)}
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
                    placeholder="branch name"
                    value={branchInput}
                    onChange={(e) => setBranchInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        addBranch();
                      }
                    }}
                  />
                  <Button type="button" size="sm" onClick={addBranch} disabled={!branchInput.trim()}>
                    <Plus className="size-4" />
                  </Button>
                </div>
              </div>
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
