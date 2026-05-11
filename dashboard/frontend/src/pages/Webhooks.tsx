import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listRepoWebhooks, createRepoWebhook, deleteRepoWebhook } from '@/lib/api';
import type { RepoWebhookInfo } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Webhook, AlertTriangle } from 'lucide-react';
import { toast } from 'sonner';

export default function Webhooks() {
  const queryClient = useQueryClient();

  const { data: repos, isLoading, error } = useQuery({
    queryKey: ['webhook-repos'],
    queryFn: listRepoWebhooks,
  });

  const enableMutation = useMutation({
    mutationFn: ({ owner, repo }: { owner: string; repo: string }) =>
      createRepoWebhook(owner, repo),
    onSuccess: (data) => {
      queryClient.setQueryData<RepoWebhookInfo[]>(['webhook-repos'], (old) =>
        old?.map((r) => r.full_name === data.full_name ? data : r),
      );
      queryClient.invalidateQueries({ queryKey: ['webhook-repos'] });
      toast.success(`Webhook enabled for ${data.full_name}`);
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const disableMutation = useMutation({
    mutationFn: ({ owner, repo, hookId }: { owner: string; repo: string; hookId: number }) =>
      deleteRepoWebhook(owner, repo, hookId),
    onSuccess: (_data, variables) => {
      const fullName = `${variables.owner}/${variables.repo}`;
      queryClient.setQueryData<RepoWebhookInfo[]>(['webhook-repos'], (old) =>
        old?.map((r) => r.full_name === fullName ? { ...r, active: false, hook_id: null } : r),
      );
      queryClient.invalidateQueries({ queryKey: ['webhook-repos'] });
      toast.success(`Webhook disabled for ${fullName}`);
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const isMutating = enableMutation.isPending || disableMutation.isPending;

  const handleToggle = (repo: RepoWebhookInfo) => {
    const [owner, name] = repo.full_name.split('/');
    if (repo.active && repo.hook_id != null) {
      disableMutation.mutate({ owner, repo: name, hookId: repo.hook_id });
    } else {
      enableMutation.mutate({ owner, repo: name });
    }
  };

  const isForbiddenError = error instanceof Error && error.message.includes('403');

  return (
    <div className="space-y-6 max-w-3xl">
      <h1 className="text-2xl font-bold">Automated Previews</h1>
      <p className="text-muted-foreground">
        When enabled, a preview environment is automatically created for every new pull request
        and kept up to date as commits are pushed. This works via a GitHub webhook that you can
        activate per repository below.
      </p>

      {isForbiddenError && (
        <div className="flex items-start gap-2 rounded-md border border-yellow-500/30 bg-yellow-500/10 px-4 py-3">
          <AlertTriangle className="size-4 mt-0.5 shrink-0 text-yellow-600 dark:text-yellow-400" />
          <p className="text-sm text-yellow-700 dark:text-yellow-300">
            GitHub returned a permission error. You may need to{' '}
            <a href="/api/auth/login" className="underline underline-offset-2 hover:text-foreground">
              re-login
            </a>{' '}
            to grant the required webhook scope.
          </p>
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Webhook className="size-5" />
            Repositories
          </CardTitle>
          <p className="text-sm text-muted-foreground">
            Only repositories where you have GitHub admin access are shown.
          </p>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-3">
              {[1, 2, 3].map((i) => (
                <div key={i} className="flex items-center justify-between py-3">
                  <div className="h-4 w-48 animate-pulse rounded bg-muted" />
                  <div className="h-8 w-20 animate-pulse rounded bg-muted" />
                </div>
              ))}
            </div>
          ) : error && !isForbiddenError ? (
            <p className="text-sm text-destructive">Failed to load repositories.</p>
          ) : !repos?.length ? (
            <p className="text-sm text-muted-foreground py-4">
              No repositories found where you have admin access.
            </p>
          ) : (
            <div className="divide-y">
              {repos.map((repo) => (
                <div key={repo.full_name} className="flex items-center justify-between py-3">
                  <div className="flex items-center gap-3">
                    <span className="text-sm font-medium">{repo.full_name}</span>
                    {repo.active ? (
                      <Badge variant="default" className="bg-green-600 hover:bg-green-600 text-white">
                        Active
                      </Badge>
                    ) : (
                      <Badge variant="secondary">Not configured</Badge>
                    )}
                  </div>
                  <Button
                    variant={repo.active ? 'outline' : 'default'}
                    size="sm"
                    disabled={isMutating}
                    onClick={() => handleToggle(repo)}
                  >
                    {repo.active ? 'Disable' : 'Enable'}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
