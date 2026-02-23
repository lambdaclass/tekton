import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listPreviews, createPreview, destroyPreview, updatePreview } from '@/lib/api';
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

  const [showCreate, setShowCreate] = useState(false);
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [slug, setSlug] = useState('');
  const [previewType, setPreviewType] = useState('node');

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

  const updateMutation = useMutation({
    mutationFn: updatePreview,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['previews'] }),
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      repo,
      branch,
      slug: slug || undefined,
      type: previewType,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Previews</h1>
        <Button
          variant={showCreate ? 'outline' : 'default'}
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
                <div className="space-y-2">
                  <Label>Type</Label>
                  <Select value={previewType} onValueChange={setPreviewType}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="node">Node.js</SelectItem>
                      <SelectItem value="vertex">Vertex (Elixir)</SelectItem>
                    </SelectContent>
                  </Select>
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
        <p className="text-muted-foreground">Loading previews...</p>
      ) : !previews?.length ? (
        <p className="text-muted-foreground">No active previews.</p>
      ) : (
        <div className="space-y-3">
          {previews.map((p) => (
            <Card key={p.slug}>
              <CardContent className="flex items-center justify-between py-4">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-3">
                    <span className="font-mono font-semibold">{p.slug}</span>
                    <Badge variant="secondary">{p.preview_type}</Badge>
                  </div>
                  <p className="text-sm text-muted-foreground mt-1">
                    {p.repo} / {p.branch}
                  </p>
                </div>
                <div className="flex items-center gap-2 ml-4">
                  <Button variant="outline" size="sm" asChild>
                    <a href={p.url} target="_blank" rel="noopener noreferrer">
                      Open
                    </a>
                  </Button>
                  <Button variant="secondary" size="sm" asChild>
                    <Link to={`/previews/${p.slug}`}>Logs</Link>
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => updateMutation.mutate(p.slug)}
                    disabled={updateMutation.isPending}
                  >
                    Update
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
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
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
