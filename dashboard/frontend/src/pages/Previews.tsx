import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listPreviews, createPreview, destroyPreview } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';

export default function Previews() {
  const queryClient = useQueryClient();

  const { data: previews, isLoading } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
    refetchInterval: 10000,
  });

  const [showCreate, setShowCreate] = useState(false);
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [slug, setSlug] = useState('');

  const createMutation = useMutation({
    mutationFn: createPreview,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['previews'] });
      setShowCreate(false);
      setRepo('');
      setBranch('');
      setSlug('');
    },
  });

  const destroyMutation = useMutation({
    mutationFn: destroyPreview,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['previews'] }),
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      repo,
      branch,
      slug: slug || undefined,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-10">
        <h1 className="text-xl font-medium tracking-tight">Previews</h1>
        <Button
          variant={showCreate ? 'outline' : 'default'}
          size="sm"
          onClick={() => setShowCreate(!showCreate)}
        >
          {showCreate ? 'Cancel' : 'Create Preview'}
        </Button>
      </div>

      {showCreate && (
        <Card className="mb-8">
          <CardHeader>
            <CardTitle>New Preview</CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleCreate}>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
                <div className="space-y-2">
                  <Label htmlFor="repo">Repository</Label>
                  <Input
                    id="repo"
                    value={repo}
                    onChange={(e) => setRepo(e.target.value)}
                    placeholder="owner/repo"
                    required
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="branch">Branch</Label>
                  <Input
                    id="branch"
                    value={branch}
                    onChange={(e) => setBranch(e.target.value)}
                    placeholder="main"
                    required
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="slug">Slug (optional)</Label>
                  <Input
                    id="slug"
                    value={slug}
                    onChange={(e) => setSlug(e.target.value)}
                    placeholder="auto-generated"
                  />
                </div>
              </div>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? 'Creating...' : 'Create'}
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
        <p className="text-sm text-muted-foreground py-8 text-center">Loading previews...</p>
      ) : !previews?.length ? (
        <p className="text-sm text-muted-foreground py-8 text-center">No active previews.</p>
      ) : (
        <div className="divide-y divide-border border rounded-md">
          {previews.map((p) => (
            <div key={p.slug} className="flex items-center justify-between px-4 py-3 hover:bg-secondary/40 transition-colors">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-3">
                  <span className="font-mono text-sm font-medium">{p.slug}</span>
                  <Badge variant="outline">{p.repo}</Badge>
                  <Badge variant="outline">{p.branch}</Badge>
                </div>
              </div>
              <div className="flex items-center gap-2 ml-4">
                <Button variant="outline" size="sm" asChild>
                  <a href={p.url} target="_blank" rel="noopener noreferrer">
                    Open
                  </a>
                </Button>
                <Button variant="outline" size="sm" asChild>
                  <Link to={`/previews/${p.slug}`}>Logs</Link>
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  onClick={() => {
                    if (confirm(`Destroy preview "${p.slug}"?`)) {
                      destroyMutation.mutate(p.slug);
                    }
                  }}
                  disabled={destroyMutation.isPending}
                >
                  Destroy
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
