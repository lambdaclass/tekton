import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listTasks, createTask } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { statusVariant } from '@/lib/status';

export default function Tasks() {
  const queryClient = useQueryClient();
  const { data: tasks, isLoading } = useQuery({
    queryKey: ['tasks'],
    queryFn: listTasks,
    refetchInterval: 5000,
  });

  const [showCreate, setShowCreate] = useState(false);
  const [prompt, setPrompt] = useState('');
  const [repo, setRepo] = useState('');
  const [baseBranch, setBaseBranch] = useState('main');

  const createMutation = useMutation({
    mutationFn: createTask,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      setShowCreate(false);
      setPrompt('');
      setRepo('');
      setBaseBranch('main');
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      prompt,
      repo,
      base_branch: baseBranch || undefined,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Claude Tasks</h1>
        <Button
          variant={showCreate ? 'outline' : 'default'}
          onClick={() => setShowCreate(!showCreate)}
        >
          {showCreate ? 'Cancel' : 'New Task'}
        </Button>
      </div>

      {showCreate && (
        <Card className="mb-8">
          <CardHeader>
            <CardTitle>New Task</CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleCreate}>
              <div className="space-y-4 mb-4">
                <div className="space-y-2">
                  <Label htmlFor="prompt">Prompt</Label>
                  <Textarea
                    id="prompt"
                    value={prompt}
                    onChange={(e) => setPrompt(e.target.value)}
                    placeholder="Describe the coding task for Claude..."
                    required
                    rows={4}
                  />
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <Label htmlFor="task-repo">Repository</Label>
                    <Input
                      id="task-repo"
                      value={repo}
                      onChange={(e) => setRepo(e.target.value)}
                      placeholder="owner/repo"
                      required
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="base-branch">Base Branch</Label>
                    <Input
                      id="base-branch"
                      value={baseBranch}
                      onChange={(e) => setBaseBranch(e.target.value)}
                      placeholder="main"
                    />
                  </div>
                </div>
              </div>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? 'Creating...' : 'Submit Task'}
              </Button>
              {createMutation.isError && (
                <p className="mt-2 text-destructive text-sm">
                  {(createMutation.error as Error).message}
                </p>
              )}
            </form>
          </CardContent>
        </Card>
      )}

      {isLoading ? (
        <p className="text-muted-foreground">Loading tasks...</p>
      ) : !tasks?.length ? (
        <p className="text-muted-foreground">No tasks yet.</p>
      ) : (
        <div className="space-y-3">
          {tasks.map((t) => (
            <Link key={t.id} to={`/tasks/${t.id}`}>
              <Card className="hover:border-muted-foreground/25 transition-colors">
                <CardContent className="py-4">
                  <div className="flex items-center justify-between mb-2">
                    <span className="font-mono text-sm text-muted-foreground">
                      {t.id.slice(0, 8)}
                    </span>
                    <Badge variant={statusVariant(t.status).variant} className={statusVariant(t.status).className}>{t.status}</Badge>
                  </div>
                  <p className="text-sm line-clamp-2 mb-2">{t.prompt}</p>
                  <div className="flex items-center gap-4 text-xs text-muted-foreground">
                    <span>{t.repo}</span>
                    <span>{t.base_branch}</span>
                    {t.preview_url && (
                      <span className="text-green-400">{t.preview_url}</span>
                    )}
                    <span className="ml-auto">
                      {new Date(t.created_at).toLocaleString()}
                    </span>
                  </div>
                </CardContent>
              </Card>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
