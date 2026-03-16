import { useMemo, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { ExternalLink } from 'lucide-react';
import { listPreviews, createPreview, destroyPreview } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

export default function Previews() {
  const queryClient = useQueryClient();

  const { data: previews, isLoading } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
    refetchInterval: 10000,
  });

  const [selectedRepo, setSelectedRepo] = useState<string>('all');
  const [showCreate, setShowCreate] = useState(false);
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [slug, setSlug] = useState('');

  const repos = useMemo(() => {
    if (!previews?.length) return [];
    return [...new Set(previews.map((p) => p.repo))].sort();
  }, [previews]);

  const filteredPreviews = useMemo(() => {
    if (!previews) return [];
    if (selectedRepo === 'all') return previews;
    return previews.filter((p) => p.repo === selectedRepo);
  }, [previews, selectedRepo]);

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
        <div className="flex items-center gap-3">
          {repos.length > 1 && (
            <Select value={selectedRepo} onValueChange={setSelectedRepo}>
              <SelectTrigger size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All repos</SelectItem>
                {repos.map((r) => (
                  <SelectItem key={r} value={r}>
                    {r}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          <Button
            variant={showCreate ? 'outline' : 'default'}
            size="sm"
            onClick={() => setShowCreate(!showCreate)}
          >
            {showCreate ? 'Cancel' : 'Create Preview'}
          </Button>
        </div>
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
      ) : !filteredPreviews.length ? (
        <p className="text-sm text-muted-foreground py-8 text-center">
          {selectedRepo !== 'all' ? 'No previews for this repo.' : 'No active previews.'}
        </p>
      ) : (
        <div className="divide-y divide-border border rounded-md">
          {filteredPreviews.map((p) => (
            <div key={p.slug} className="px-4 py-3 hover:bg-secondary/40 transition-colors">
              <div className="flex items-center justify-between">
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
              {p.extra_urls?.length > 0 && (
                <div className="flex flex-wrap gap-2 mt-2 ml-1">
                  {p.extra_urls.map((eu) => (
                    <a
                      key={eu.label}
                      href={eu.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
                    >
                      {eu.label}
                      <ExternalLink className="size-3" />
                    </a>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
