import { useState, useRef, useEffect, useCallback } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router-dom';
import { listTasks, createTask, listRepos, uploadImage, getMe, type ListTasksParams } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { statusVariant } from '@/lib/status';
import { timeAgo, formatCost } from '@/lib/utils';
import VoiceInput from '@/components/VoiceInput';
import BranchCombobox from '@/components/BranchCombobox';
import { ImagePlus, X, ChevronLeft, ChevronRight, Search, BrainCircuit, LayoutGrid, List } from 'lucide-react';

type ViewMode = 'card' | 'list';
const VIEW_STORAGE_KEY = 'tekton-task-view';

const STATUS_OPTIONS = ['all', 'pending', 'creating_agent', 'cloning', 'running_claude', 'pushing', 'creating_preview', 'awaiting_followup', 'completed', 'failed'];

function SkeletonCard() {
  return (
    <Card>
      <CardContent className="py-4">
        {/* ID + status badge row */}
        <div className="flex items-center justify-between mb-2">
          <Skeleton className="h-4 w-16 rounded" />
          <Skeleton className="h-5 w-24 rounded-full" />
        </div>
        {/* Task name */}
        <Skeleton className="h-4 w-2/3 mb-1.5 rounded" />
        {/* Prompt (two lines) */}
        <Skeleton className="h-3.5 w-full mb-1 rounded" />
        <Skeleton className="h-3.5 w-4/5 mb-3 rounded" />
        {/* Footer: repo, branch, cost, avatar + time */}
        <div className="flex items-center gap-4">
          <Skeleton className="h-3 w-28 rounded" />
          <Skeleton className="h-3 w-14 rounded" />
          <Skeleton className="h-3 w-12 rounded" />
          <div className="ml-auto flex items-center gap-1.5">
            <Skeleton className="size-4 rounded-full" />
            <Skeleton className="h-3 w-10 rounded" />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function SkeletonRow() {
  return (
    <div className="flex items-center gap-3 px-3 py-2.5 border-t border-border first:border-t-0">
      <Skeleton className="size-4 rounded-full shrink-0" />
      <Skeleton className="h-4 flex-1 rounded" />
      <Skeleton className="h-4 w-16 rounded" />
      <Skeleton className="h-3 w-14 rounded" />
      <Skeleton className="size-5 rounded-full shrink-0" />
      <Skeleton className="h-3 w-12 rounded" />
    </div>
  );
}

export default function Tasks() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });

  // Filter / pagination state
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('all');
  const [page, setPage] = useState(1);
  const perPage = 50;

  // View mode (card / list)
  const [viewMode, setViewMode] = useState<ViewMode>(() => {
    const stored = localStorage.getItem(VIEW_STORAGE_KEY);
    return stored === 'list' ? 'list' : 'card';
  });
  const toggleView = (mode: ViewMode) => {
    setViewMode(mode);
    localStorage.setItem(VIEW_STORAGE_KEY, mode);
  };

  // Keyboard navigation
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const cardRefs = useRef<(HTMLAnchorElement | null)[]>([]);

  // Debounce search input
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      setSearch(searchInput);
      setPage(1);
      setSelectedIndex(-1);
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
  const [customBranch, setCustomBranch] = useState('');
  const [uploading, setUploading] = useState(false);
  const imageInputRef = useRef<HTMLInputElement>(null);

  const { data: knownRepos } = useQuery({
    queryKey: ['repos'],
    queryFn: listRepos,
  });

  // Keyboard shortcuts
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

    if (e.key === 'n') {
      e.preventDefault();
      setShowCreate((prev) => !prev);
      return;
    }
    if (e.key === 'Escape') {
      setShowCreate(false);
      return;
    }
    if (!tasks?.length) return;

    if (e.key === 'j') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.min(prev + 1, tasks.length - 1));
    } else if (e.key === 'k') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.max(prev - 1, 0));
    } else if (e.key === 'Enter' && selectedIndex >= 0 && selectedIndex < tasks.length) {
      e.preventDefault();
      navigate(`/tasks/${tasks[selectedIndex].id}`);
    }
  }, [tasks, selectedIndex, navigate]);

  useEffect(() => {
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  // Auto-scroll selected card into view
  useEffect(() => {
    if (selectedIndex >= 0 && cardRefs.current[selectedIndex]) {
      cardRefs.current[selectedIndex]?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }
  }, [selectedIndex]);

  // Selection resets are co-located with the state changes below

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
    mutationFn: async (data: { prompt: string; repo: string; base_branch?: string; image_urls?: string[]; custom_branch_name?: string }) => {
      return createTask(data);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      setShowCreate(false);
      setPrompt('');
      setRepo('');
      setBaseBranch('main');
      setCustomBranch('');
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
      custom_branch_name: customBranch || undefined,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Tasks</h1>
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
                      placeholder="Describe the coding task..."
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
                <div className="space-y-2">
                  <Label htmlFor="custom-branch">Branch Name (optional)</Label>
                  <Input
                    id="custom-branch"
                    value={customBranch}
                    onChange={(e) => setCustomBranch(e.target.value)}
                    placeholder="Auto-generated from task name if left blank"
                  />
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
          onChange={(e) => { setStatusFilter(e.target.value); setPage(1); setSelectedIndex(-1); }}
          className="h-9 rounded-md border border-input bg-background px-3 text-sm"
        >
          {STATUS_OPTIONS.map((s) => (
            <option key={s} value={s}>{s === 'all' ? 'All statuses' : s}</option>
          ))}
        </select>
        <div className="flex rounded-md border border-input overflow-hidden">
          <button
            onClick={() => toggleView('card')}
            className={`flex items-center justify-center size-9 transition-colors ${viewMode === 'card' ? 'bg-accent text-accent-foreground' : 'bg-background text-muted-foreground hover:text-foreground'}`}
            title="Card view"
          >
            <LayoutGrid className="size-4" />
          </button>
          <button
            onClick={() => toggleView('list')}
            className={`flex items-center justify-center size-9 border-l border-input transition-colors ${viewMode === 'list' ? 'bg-accent text-accent-foreground' : 'bg-background text-muted-foreground hover:text-foreground'}`}
            title="List view"
          >
            <List className="size-4" />
          </button>
        </div>
      </div>

      {isLoading ? (
        viewMode === 'card' ? (
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-3">
            {Array.from({ length: 6 }).map((_, i) => (
              <SkeletonCard key={i} />
            ))}
          </div>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            {Array.from({ length: 8 }).map((_, i) => (
              <SkeletonRow key={i} />
            ))}
          </div>
        )
      ) : !tasks?.length ? (
        <div className="flex flex-col items-center justify-center py-16 text-muted-foreground">
          <BrainCircuit className="size-12 mb-4 opacity-50" />
          <p className="text-lg font-medium mb-1">
            {search || statusFilter !== 'all' ? 'No matching tasks' : 'No tasks yet'}
          </p>
          <p className="text-sm">
            {search || statusFilter !== 'all'
              ? 'Try adjusting your search or filters'
              : 'Create a new task to get started'}
          </p>
        </div>
      ) : viewMode === 'card' ? (
        <div className="grid grid-cols-1 xl:grid-cols-2 gap-3">
          {tasks.map((t, index) => {
            const sv = statusVariant(t.status);
            const StatusIcon = sv.icon;
            return (
              <Link
                key={t.id}
                to={`/tasks/${t.id}`}
                ref={(el) => { cardRefs.current[index] = el; }}
              >
                <Card className={`hover:bg-secondary/30 transition-colors ${index === selectedIndex ? 'ring-1 ring-ring' : ''}`}>
                  <CardContent className="py-4">
                    <div className="flex items-center justify-between mb-2">
                      <span className="font-mono text-sm text-muted-foreground">
                        {t.id.slice(0, 8)}
                      </span>
                      <Badge variant={sv.variant} className={sv.className}>
                        {StatusIcon && <StatusIcon className={sv.spin ? 'animate-spin' : ''} />}
                        {t.status}
                      </Badge>
                    </div>
                    {t.name && (
                      <p className="text-sm font-medium mb-1">{t.name}</p>
                    )}
                    <p className="text-sm line-clamp-2 mb-2 text-muted-foreground">{t.prompt}</p>
                    <div className="flex items-center gap-4 text-xs text-muted-foreground min-w-0">
                      <span className="shrink-0">{t.repo}</span>
                      <span className="shrink-0">{t.base_branch}</span>
                      {t.pr_url && (
                        <a
                          href={t.pr_url}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(e) => e.stopPropagation()}
                          className="shrink-0 text-blue-400 hover:text-blue-300"
                        >
                          PR #{t.pr_number}
                        </a>
                      )}
                      {t.preview_url && (
                        <a
                          href={t.preview_url}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(e) => e.stopPropagation()}
                          className="text-green-400 hover:text-green-300 truncate min-w-0"
                          title={t.preview_url}
                        >
                          {t.preview_url.replace(/^https?:\/\//, '')}
                        </a>
                      )}
                      {t.total_cost_usd ? (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span className="shrink-0 cursor-default tabular-nums">{formatCost(t.total_cost_usd)}</span>
                          </TooltipTrigger>
                          <TooltipContent>
                            {(t.total_input_tokens ?? 0).toLocaleString()} input + {(t.total_output_tokens ?? 0).toLocaleString()} output tokens
                          </TooltipContent>
                        </Tooltip>
                      ) : null}
                      <span className="ml-auto shrink-0 flex items-center gap-1.5">
                        {t.created_by && (
                          <img
                            src={`https://github.com/${t.created_by}.png?size=20`}
                            className="size-4 rounded-full"
                            loading="lazy"
                            alt=""
                          />
                        )}
                        <span title={new Date(t.created_at).toLocaleString()}>
                          {timeAgo(t.created_at)}
                        </span>
                      </span>
                    </div>
                  </CardContent>
                </Card>
              </Link>
            );
          })}
        </div>
      ) : (
        /* List view */
        <div className="rounded-lg border border-border overflow-hidden">
          {tasks.map((t, index) => {
            const sv = statusVariant(t.status);
            const StatusIcon = sv.icon;
            return (
              <Link
                key={t.id}
                to={`/tasks/${t.id}`}
                ref={(el) => { cardRefs.current[index] = el; }}
                className={`flex items-center gap-3 px-3 py-2.5 text-sm hover:bg-secondary/40 transition-colors ${
                  index === selectedIndex ? 'bg-accent/70' : ''
                } ${index > 0 ? 'border-t border-border' : ''}`}
              >
                {StatusIcon && (
                  <StatusIcon
                    className={`size-4 shrink-0 ${sv.spin ? 'animate-spin' : ''} ${
                      sv.className?.includes('text-') ? sv.className.split(' ').find(c => c.startsWith('text-')) : 'text-muted-foreground'
                    }`}
                  />
                )}
                <span className="truncate flex-1 min-w-0">
                  {t.name || t.prompt}
                </span>
                <Badge variant="outline" className="shrink-0 text-[10px] px-1.5 py-0">
                  {t.repo.split('/').pop()}
                </Badge>
                {t.total_cost_usd ? (
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums w-14 text-right">
                    {formatCost(t.total_cost_usd)}
                  </span>
                ) : (
                  <span className="shrink-0 w-14" />
                )}
                {t.created_by && (
                  <img
                    src={`https://github.com/${t.created_by}.png?size=20`}
                    className="size-5 rounded-full shrink-0"
                    loading="lazy"
                    alt=""
                  />
                )}
                <span className="shrink-0 text-xs text-muted-foreground w-16 text-right" title={new Date(t.created_at).toLocaleString()}>
                  {timeAgo(t.created_at)}
                </span>
              </Link>
            );
          })}
        </div>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-center gap-4 mt-6">
          <Button
            variant="outline"
            size="sm"
            onClick={() => { setPage((p) => Math.max(1, p - 1)); setSelectedIndex(-1); }}
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
            onClick={() => { setPage((p) => Math.min(totalPages, p + 1)); setSelectedIndex(-1); }}
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
