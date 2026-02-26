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
  getMe,
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
import { Users, KeyRound, Trash2, Plus, X, Settings } from 'lucide-react';
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
      <UsersSection queryClient={queryClient} />
      <SecretsSection queryClient={queryClient} />
    </div>
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
