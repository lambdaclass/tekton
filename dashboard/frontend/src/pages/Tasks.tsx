import { useState, useRef, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listTasks, createTask, listRepos, uploadImage, getMe, type ListTasksParams } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { statusVariant } from '@/lib/status';
import VoiceInput from '@/components/VoiceInput';
import BranchCombobox from '@/components/BranchCombobox';
import { ImagePlus, X, ChevronLeft, ChevronRight, Search } from 'lucide-react';

const STATUS_OPTIONS = ['all', 'pending', 'creating_agent', 'cloning', 'running_claude', 'pushing', 'creating_preview', 'awaiting_followup', 'completed', 'failed'];

export default function Tasks() {
  const queryClient = useQueryClient();
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });

  // Filter / pagination state
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('all');
  const [page, setPage] = useState(1);
  const perPage = 50;

  // Debounce search input
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      setSearch(searchInput);
      setPage(1);
    }, 400);
    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [searchInput]);

  const queryParams: ListTasksParams = {
    page,
    per_page: perPage,
    ...(statusFilter !== 'all' ? { status: statusFilter } : {}),
    ...(search ? { search } : {}),
  };

  const { data, isLoading } = useQuery({
    queryKey: ['tasks', queryParams],
    queryFn: () => listTasks(queryParams),
    refetchInterval: 5000,
  });

  const tasks = data?.tasks;
  const total = data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / perPage));

  const [showCreate, setShowCreate] = useState(false);
  const [prompt, setPrompt] = useState('');
  const [repo, setRepo] = useState('');
  const [baseBranch, setBaseBranch] = useState('main');
  const [imageFiles, setImageFiles] = useState<File[]>([]);
  const [imagePreviews, setImagePreviews] = useState<string[]>([]);
  const [uploading, setUploading] = useState(false);
  const imageInputRef = useRef<HTMLInputElement>(null);

  const { data: knownRepos } = useQuery({
    queryKey: ['repos'],
    queryFn: listRepos,
  });

  const handlePromptChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setPrompt(e.target.value);
  };

  const handleTranscript = (text: string) => {
    const next = prompt ? `${prompt} ${text}` : text;
    setPrompt(next);
  };

  const addImageFiles = (files: FileList | File[]) => {
    const newFiles = Array.from(files).filter((f) => f.type.startsWith('image/'));
    if (newFiles.length === 0) return;
    setImageFiles((prev) => [...prev, ...newFiles]);
    for (const file of newFiles) {
      const reader = new FileReader();
      reader.onload = (e) =>
        setImagePreviews((prev) => [...prev, e.target?.result as string]);
      reader.readAsDataURL(file);
    }
  };

  const removeImage = (index: number) => {
    setImageFiles((prev) => prev.filter((_, i) => i !== index));
    setImagePreviews((prev) => prev.filter((_, i) => i !== index));
  };

  const handleImageDrop = (e: React.DragEvent) => {
    e.preventDefault();
    addImageFiles(e.dataTransfer.files);
  };

  const createMutation = useMutation({
    mutationFn: async (data: { prompt: string; repo: string; base_branch?: string; image_urls?: string[] }) => {
      return createTask(data);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      setShowCreate(false);
      setPrompt('');
      setRepo('');
      setBaseBranch('main');
      setImageFiles([]);
      setImagePreviews([]);
    },
  });

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    let image_urls: string[] | undefined;

    if (imageFiles.length > 0) {
      setUploading(true);
      try {
        const results = await Promise.all(imageFiles.map((f) => uploadImage(f)));
        image_urls = results.map((r) => r.url);
      } catch {
        setUploading(false);
        return;
      }
      setUploading(false);
    }

    createMutation.mutate({
      prompt,
      repo,
      base_branch: baseBranch || undefined,
      image_urls,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Claude Tasks</h1>
        {me?.role !== 'viewer' && (
          <Button
            variant={showCreate ? 'outline' : 'default'}
            onClick={() => setShowCreate(!showCreate)}
          >
            {showCreate ? 'Cancel' : 'New Task'}
          </Button>
        )}
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
                  <div className="flex gap-2 items-start">
                    <Textarea
                      id="prompt"
                      value={prompt}
                      onChange={handlePromptChange}
                      placeholder="Describe the coding task for Claude..."
                      required
                      rows={4}
                      className="flex-1"
                    />
                    <VoiceInput onTranscript={handleTranscript} />
                  </div>
                </div>
                <div className="space-y-2">
                  <Label>Image Attachments</Label>
                  <div
                    onDragOver={(e) => e.preventDefault()}
                    onDrop={handleImageDrop}
                    className="border-2 border-dashed border-border rounded-lg p-4 text-center hover:border-muted-foreground/50 transition-colors cursor-pointer"
                    onClick={() => imageInputRef.current?.click()}
                  >
                    <input
                      ref={imageInputRef}
                      type="file"
                      accept="image/png,image/jpeg,image/gif,image/webp"
                      multiple
                      className="hidden"
                      onChange={(e) => {
                        if (e.target.files?.length) addImageFiles(e.target.files);
                        e.target.value = '';
                      }}
                    />
                    {imagePreviews.length > 0 ? (
                      <div className="flex flex-wrap gap-3 justify-center">
                        {imagePreviews.map((preview, i) => (
                          <div key={i} className="relative inline-block">
                            <img
                              src={preview}
                              alt={`Upload preview ${i + 1}`}
                              className="max-h-32 rounded-md border border-border"
                            />
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation();
                                removeImage(i);
                              }}
                              className="absolute -top-2 -right-2 bg-destructive text-destructive-foreground rounded-full p-0.5"
                            >
                              <X className="size-3" />
                            </button>
                          </div>
                        ))}
                        <div className="flex items-center justify-center w-20 h-20 rounded-md border-2 border-dashed border-border text-muted-foreground hover:border-muted-foreground/50">
                          <ImagePlus className="size-6" />
                        </div>
                      </div>
                    ) : (
                      <div className="flex flex-col items-center gap-2 text-muted-foreground">
                        <ImagePlus className="size-8" />
                        <span className="text-sm">Drop images here or click to select</span>
                        <span className="text-xs">PNG, JPG, GIF, WebP (max 10MB each)</span>
                      </div>
                    )}
                  </div>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <Label htmlFor="task-repo">Repository</Label>
                    <Input
                      id="task-repo"
                      list="repo-options"
                      value={repo}
                      onChange={(e) => setRepo(e.target.value)}
                      placeholder="owner/repo"
                      required
                    />
                    <datalist id="repo-options">
                      {knownRepos?.map((r) => (
                        <option key={r} value={r} />
                      ))}
                    </datalist>
                  </div>
                  <div className="space-y-2">
                    <Label>Base Branch</Label>
                    <BranchCombobox
                      repo={repo}
                      value={baseBranch}
                      onChange={setBaseBranch}
                    />
                  </div>
                </div>
              </div>
              <Button type="submit" disabled={createMutation.isPending || uploading}>
                {uploading ? 'Uploading images...' : createMutation.isPending ? 'Creating...' : 'Submit Task'}
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

      {/* Search and filter bar */}
      <div className="flex flex-col sm:flex-row gap-3 mb-4">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
          <Input
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder="Search prompts..."
            className="pl-9"
          />
        </div>
        <select
          value={statusFilter}
          onChange={(e) => { setStatusFilter(e.target.value); setPage(1); }}
          className="h-9 rounded-md border border-input bg-background px-3 text-sm"
        >
          {STATUS_OPTIONS.map((s) => (
            <option key={s} value={s}>{s === 'all' ? 'All statuses' : s}</option>
          ))}
        </select>
      </div>

      {isLoading ? (
        <p className="text-muted-foreground">Loading tasks...</p>
      ) : !tasks?.length ? (
        <p className="text-muted-foreground">No tasks found.</p>
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
                  {t.name && (
                    <p className="text-sm font-medium mb-1">{t.name}</p>
                  )}
                  <p className="text-sm line-clamp-2 mb-2 text-muted-foreground">{t.prompt}</p>
                  <div className="flex items-center gap-4 text-xs text-muted-foreground">
                    <span>{t.repo}</span>
                    <span>{t.base_branch}</span>
                    {t.pr_url && (
                      <a
                        href={t.pr_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        onClick={(e) => e.stopPropagation()}
                        className="text-blue-400 hover:text-blue-300"
                      >
                        PR #{t.pr_number}
                      </a>
                    )}
                    {t.preview_url && (
                      <span className="text-green-400">{t.preview_url}</span>
                    )}
                    {(t.total_input_tokens || t.total_output_tokens) ? (
                      <span>{((t.total_input_tokens ?? 0) + (t.total_output_tokens ?? 0)).toLocaleString()} tokens</span>
                    ) : null}
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

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-center gap-4 mt-6">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page <= 1}
          >
            <ChevronLeft className="size-4" />
            Prev
          </Button>
          <span className="text-sm text-muted-foreground">
            Page {page} of {totalPages} ({total} tasks)
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
          >
            Next
            <ChevronRight className="size-4" />
          </Button>
        </div>
      )}
    </div>
  );
}
