import { useState, useMemo, useRef, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link, Navigate } from 'react-router-dom';
import {
  DragDropContext,
  Droppable,
  Draggable,
  type DropResult,
} from '@hello-pangea/dnd';
import {
  listAllIntakeIssues,
  updateIntakeIssueStatus,
  getMe,
  type IntakeIssueWithMeta,
} from '@/lib/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { cn, timeAgo } from '@/lib/utils';
import {
  Search,
  ExternalLink,
  AlertTriangle,
  GitBranch,
  Inbox,
  Clock,
  Play,
  Eye,
  CheckCircle2,
  XCircle,
} from 'lucide-react';

// ---------------------------------------------------------------------------
// Transition rules (must match backend valid_transitions)
// ---------------------------------------------------------------------------

const VALID_TRANSITIONS: Record<string, string[]> = {
  backlog: ['pending', 'done'],
  pending: ['backlog'],
  task_created: ['failed'],
  review: ['done', 'failed'],
  failed: ['backlog', 'pending'],
  done: [],
};

// ---------------------------------------------------------------------------
// Column definitions
// ---------------------------------------------------------------------------

interface ColumnDef {
  id: string;
  label: string;
  icon: React.ElementType;
  headerColor: string;
  dotColor: string;
}

const COLUMNS: ColumnDef[] = [
  {
    id: 'backlog',
    label: 'Backlog',
    icon: Inbox,
    headerColor: 'text-zinc-500 dark:text-zinc-400',
    dotColor: 'bg-zinc-400 dark:bg-zinc-500',
  },
  {
    id: 'pending',
    label: 'Pending',
    icon: Clock,
    headerColor: 'text-yellow-600 dark:text-yellow-400',
    dotColor: 'bg-yellow-500 dark:bg-yellow-400',
  },
  {
    id: 'task_created',
    label: 'In Progress',
    icon: Play,
    headerColor: 'text-blue-600 dark:text-blue-400',
    dotColor: 'bg-blue-500 dark:bg-blue-400',
  },
  {
    id: 'review',
    label: 'Review',
    icon: Eye,
    headerColor: 'text-amber-600 dark:text-amber-400',
    dotColor: 'bg-amber-500 dark:bg-amber-400',
  },
  {
    id: 'done',
    label: 'Done',
    icon: CheckCircle2,
    headerColor: 'text-emerald-600 dark:text-emerald-400',
    dotColor: 'bg-emerald-500 dark:bg-emerald-400',
  },
  {
    id: 'failed',
    label: 'Failed',
    icon: XCircle,
    headerColor: 'text-red-600 dark:text-red-400',
    dotColor: 'bg-red-500 dark:bg-red-400',
  },
];

// ---------------------------------------------------------------------------
// Label color helpers
// ---------------------------------------------------------------------------

const LABEL_COLORS = [
  'bg-violet-500/15 text-violet-700 dark:text-violet-300 border-violet-500/20',
  'bg-sky-500/15 text-sky-700 dark:text-sky-300 border-sky-500/20',
  'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 border-emerald-500/20',
  'bg-amber-500/15 text-amber-700 dark:text-amber-300 border-amber-500/20',
  'bg-rose-500/15 text-rose-700 dark:text-rose-300 border-rose-500/20',
  'bg-teal-500/15 text-teal-700 dark:text-teal-300 border-teal-500/20',
  'bg-fuchsia-500/15 text-fuchsia-700 dark:text-fuchsia-300 border-fuchsia-500/20',
  'bg-orange-500/15 text-orange-700 dark:text-orange-300 border-orange-500/20',
];

function labelColor(label: string): string {
  let hash = 0;
  for (let i = 0; i < label.length; i++) {
    hash = label.charCodeAt(i) + ((hash << 5) - hash);
  }
  return LABEL_COLORS[Math.abs(hash) % LABEL_COLORS.length];
}

// ---------------------------------------------------------------------------
// Skeleton loader
// ---------------------------------------------------------------------------

function BoardSkeleton() {
  return (
    <div className="grid grid-cols-6 gap-2 flex-1">
      {COLUMNS.map((col) => (
        <div
          key={col.id}
          className="rounded-xl bg-muted/40 border border-border/50 p-2"
        >
          <div className="flex items-center gap-2 mb-3 px-1">
            <Skeleton className="h-4 w-16 rounded" />
            <Skeleton className="h-5 w-6 rounded-full ml-auto" />
          </div>
          <div className="space-y-2">
            {Array.from({ length: col.id === 'backlog' ? 3 : 2 }).map(
              (_, i) => (
                <div
                  key={i}
                  className="rounded-lg bg-background border border-border/50 p-2.5 space-y-2"
                >
                  <Skeleton className="h-3.5 w-4/5 rounded" />
                  <Skeleton className="h-3 w-2/3 rounded" />
                  <div className="flex gap-1">
                    <Skeleton className="h-3.5 w-10 rounded-full" />
                    <Skeleton className="h-3.5 w-12 rounded-full" />
                  </div>
                </div>
              ),
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Issue detail dialog
// ---------------------------------------------------------------------------

function IssueDetailDialog({
  issue,
  onClose,
  onStatusChange,
}: {
  issue: IntakeIssueWithMeta;
  onClose: () => void;
  onStatusChange: (status: string) => void;
}) {
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="text-base leading-snug pr-6">
            {issue.external_title}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 text-sm">
          {/* Meta row */}
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground">
            <span className="flex items-center gap-1 font-mono text-xs">
              <GitBranch className="size-3" />
              {issue.source_repo}
            </span>
            <span>{issue.source_name}</span>
            <span>{timeAgo(issue.updated_at)}</span>
          </div>

          {/* Labels */}
          {issue.external_labels.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {issue.external_labels.map((lbl) => (
                <span
                  key={lbl}
                  className={cn(
                    'inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium',
                    labelColor(lbl),
                  )}
                >
                  {lbl}
                </span>
              ))}
            </div>
          )}

          {/* Body */}
          {issue.external_body && (
            <div className="rounded-lg border bg-muted/30 p-3 max-h-60 overflow-y-auto">
              <pre className="whitespace-pre-wrap text-xs leading-relaxed font-sans">
                {issue.external_body}
              </pre>
            </div>
          )}

          {/* Error message */}
          {issue.error_message && (
            <div className="rounded-lg border border-red-500/20 bg-red-500/5 p-3 text-xs text-red-600 dark:text-red-400">
              <div className="flex items-center gap-1.5 font-medium mb-1">
                <AlertTriangle className="size-3.5" />
                Error
              </div>
              {issue.error_message}
            </div>
          )}

          {/* Links */}
          <div className="flex flex-wrap gap-2">
            {issue.external_url && (
              <Button variant="outline" size="sm" asChild>
                <a
                  href={issue.external_url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <ExternalLink className="size-3.5 mr-1.5" />
                  View External Issue
                </a>
              </Button>
            )}
            {issue.task_id && (
              <Button variant="outline" size="sm" asChild>
                <Link to={`/tasks/${issue.task_id}`}>
                  <Play className="size-3.5 mr-1.5" />
                  View Task {issue.task_id.slice(0, 8)}
                  {issue.task_status && (
                    <Badge variant="secondary" className="ml-1.5 text-[10px]">
                      {issue.task_status}
                    </Badge>
                  )}
                </Link>
              </Button>
            )}
          </div>

          {/* Status changer — only show valid transitions */}
          <div>
            <p className="text-xs text-muted-foreground mb-1.5">Move to:</p>
            <div className="flex flex-wrap gap-1.5">
              {COLUMNS.filter(
                (col) =>
                  col.id === issue.status ||
                  (VALID_TRANSITIONS[issue.status] ?? []).includes(col.id),
              ).map((col) => (
                <Button
                  key={col.id}
                  variant={issue.status === col.id ? 'default' : 'outline'}
                  size="sm"
                  className="text-xs h-7"
                  disabled={issue.status === col.id}
                  onClick={() => onStatusChange(col.id)}
                >
                  <span className={cn('size-1.5 rounded-full mr-1.5', col.dotColor)} />
                  {col.label}
                </Button>
              ))}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Issue card
// ---------------------------------------------------------------------------

interface IssueCardProps {
  issue: IntakeIssueWithMeta;
  index: number;
  onSelect: (issue: IntakeIssueWithMeta) => void;
}

function IssueCard({ issue, index, onSelect }: IssueCardProps) {
  return (
    <Draggable draggableId={String(issue.id)} index={index}>
      {(provided, snapshot) => (
        <div
          ref={provided.innerRef}
          {...provided.draggableProps}
          {...provided.dragHandleProps}
          onClick={() => onSelect(issue)}
          className={cn(
            'rounded-lg bg-background border border-border/60 p-2.5 transition-shadow select-none cursor-pointer',
            'hover:border-border hover:shadow-sm',
            snapshot.isDragging &&
              'shadow-lg ring-2 ring-primary/20 rotate-[1.5deg] scale-[1.02]',
          )}
        >
          {/* Title */}
          <p className="text-[13px] font-medium leading-snug line-clamp-2 mb-1">
            {issue.external_title}
          </p>

          {/* Repo */}
          <div className="flex items-center gap-1 mb-1.5 text-[11px] text-muted-foreground overflow-hidden">
            <GitBranch className="size-2.5 shrink-0" />
            <span className="truncate font-mono">{issue.source_repo}</span>
            {issue.external_url && (
              <a
                href={issue.external_url}
                target="_blank"
                rel="noopener noreferrer"
                onClick={(e) => e.stopPropagation()}
                className="shrink-0 hover:text-foreground transition-colors ml-auto"
                title="Open external issue"
              >
                <ExternalLink className="size-2.5" />
              </a>
            )}
          </div>

          {/* Labels */}
          {issue.external_labels.length > 0 && (
            <div className="flex flex-wrap gap-0.5 mb-1.5">
              {issue.external_labels.slice(0, 3).map((lbl) => (
                <span
                  key={lbl}
                  className={cn(
                    'inline-flex items-center rounded-full border px-1.5 text-[10px] font-medium leading-4',
                    labelColor(lbl),
                  )}
                >
                  {lbl}
                </span>
              ))}
              {issue.external_labels.length > 3 && (
                <span className="text-[10px] text-muted-foreground">
                  +{issue.external_labels.length - 3}
                </span>
              )}
            </div>
          )}

          {/* Footer */}
          <div className="flex items-center justify-between gap-1 text-[11px] text-muted-foreground">
            <div className="flex items-center gap-1.5 min-w-0">
              {issue.task_id && (
                <Link
                  to={`/tasks/${issue.task_id}`}
                  onClick={(e) => e.stopPropagation()}
                  className="font-mono text-blue-600 dark:text-blue-400 hover:underline"
                  title={`Task ${issue.task_id}`}
                >
                  {issue.task_id.slice(0, 6)}
                </Link>
              )}
              {issue.error_message && (
                <AlertTriangle className="size-3 text-red-500 dark:text-red-400 shrink-0" />
              )}
            </div>
            <span className="shrink-0" title={new Date(issue.updated_at).toLocaleString()}>
              {timeAgo(issue.updated_at)}
            </span>
          </div>
        </div>
      )}
    </Draggable>
  );
}

// ---------------------------------------------------------------------------
// Column component
// ---------------------------------------------------------------------------

interface BoardColumnProps {
  column: ColumnDef;
  issues: IntakeIssueWithMeta[];
  onSelect: (issue: IntakeIssueWithMeta) => void;
}

function BoardColumn({ column, issues, onSelect }: BoardColumnProps) {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listRef.current?.scrollTo({ top: 0, behavior: 'smooth' });
  }, [issues.length]);

  return (
    <div className="flex flex-col rounded-xl bg-muted/40 border border-border/50 overflow-hidden">
      {/* Column header */}
      <div className="flex items-center gap-1.5 px-2.5 pt-2.5 pb-1.5">
        <span className={cn('size-2 rounded-full', column.dotColor)} />
        <span className={cn('text-xs font-semibold', column.headerColor)}>
          {column.label}
        </span>
        <Badge
          variant="secondary"
          className="ml-auto text-[10px] px-1.5 py-0 h-4 tabular-nums"
        >
          {issues.length}
        </Badge>
      </div>

      {/* Drop zone */}
      <Droppable droppableId={column.id}>
        {(provided, snapshot) => (
          <div
            ref={(el) => {
              provided.innerRef(el);
              (listRef as React.MutableRefObject<HTMLDivElement | null>).current = el;
            }}
            {...provided.droppableProps}
            className={cn(
              'flex-1 overflow-y-auto px-1.5 pb-1.5 space-y-1.5 min-h-[80px] transition-colors',
              snapshot.isDraggingOver && 'bg-primary/5',
            )}
          >
            {issues.map((issue, idx) => (
              <IssueCard key={issue.id} issue={issue} index={idx} onSelect={onSelect} />
            ))}
            {provided.placeholder}
            {issues.length === 0 && !snapshot.isDraggingOver && (
              <div className="flex items-center justify-center h-16 text-[11px] text-muted-foreground/40">
                No issues
              </div>
            )}
          </div>
        )}
      </Droppable>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main board page
// ---------------------------------------------------------------------------

export default function IntakeBoard() {
  const queryClient = useQueryClient();
  const { data: me, isLoading: meLoading } = useQuery({ queryKey: ['me'], queryFn: getMe });

  const { data: issues, isLoading } = useQuery({
    queryKey: ['intake-issues-all'],
    queryFn: listAllIntakeIssues,
    refetchInterval: 5000,
  });

  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [sourceFilter, setSourceFilter] = useState('all');
  const [selectedIssue, setSelectedIssue] = useState<IntakeIssueWithMeta | null>(null);

  // Debounce search
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => setSearch(searchInput), 300);
    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [searchInput]);

  const sources = useMemo(() => {
    if (!issues) return [];
    const map = new Map<number, { name: string; repo: string }>();
    for (const issue of issues) {
      if (!map.has(issue.source_id)) {
        map.set(issue.source_id, { name: issue.source_name, repo: issue.source_repo });
      }
    }
    return Array.from(map.entries()).sort((a, b) => a[1].name.localeCompare(b[1].name));
  }, [issues]);

  const filtered = useMemo(() => {
    if (!issues) return [];
    return issues.filter((issue) => {
      if (sourceFilter !== 'all' && String(issue.source_id) !== sourceFilter) return false;
      if (search && !issue.external_title.toLowerCase().includes(search.toLowerCase())) return false;
      return true;
    });
  }, [issues, sourceFilter, search]);

  const columnData = useMemo(() => {
    const map = new Map<string, IntakeIssueWithMeta[]>();
    for (const col of COLUMNS) map.set(col.id, []);
    for (const issue of filtered) {
      const bucket = map.get(issue.status);
      if (bucket) bucket.push(issue);
      else map.get('backlog')!.push(issue);
    }
    for (const arr of map.values()) {
      arr.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime());
    }
    return map;
  }, [filtered]);

  const statusMutation = useMutation({
    mutationFn: ({ id, status }: { id: number; status: string }) =>
      updateIntakeIssueStatus(id, status),
    onMutate: async ({ id, status }) => {
      await queryClient.cancelQueries({ queryKey: ['intake-issues-all'] });
      const prev = queryClient.getQueryData<IntakeIssueWithMeta[]>(['intake-issues-all']);
      queryClient.setQueryData<IntakeIssueWithMeta[]>(
        ['intake-issues-all'],
        (old) => old?.map((issue) => (issue.id === id ? { ...issue, status } : issue)),
      );
      return { prev };
    },
    onError: (_err, _vars, context) => {
      if (context?.prev) queryClient.setQueryData(['intake-issues-all'], context.prev);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['intake-issues-all'] });
    },
  });

  const onDragEnd = (result: DropResult) => {
    const { draggableId, destination } = result;
    if (!destination) return;
    const newStatus = destination.droppableId;
    const issueId = Number(draggableId);
    const issue = issues?.find((i) => i.id === issueId);
    if (!issue || issue.status === newStatus) return;
    // Validate transition client-side — invalid drops snap back silently
    const allowed = VALID_TRANSITIONS[issue.status] ?? [];
    if (!allowed.includes(newStatus)) return;
    statusMutation.mutate({ id: issueId, status: newStatus });
  };

  const handleStatusChange = (status: string) => {
    if (!selectedIssue || selectedIssue.status === status) return;
    const allowed = VALID_TRANSITIONS[selectedIssue.status] ?? [];
    if (!allowed.includes(status)) return;
    statusMutation.mutate({ id: selectedIssue.id, status });
    setSelectedIssue({ ...selectedIssue, status });
  };

  const totalCount = issues?.length ?? 0;

  // Concurrency indicator: count issues holding slots (task_created + review)
  const slotsInUse = useMemo(() => {
    if (!issues) return 0;
    return issues.filter(
      (i) => i.status === 'task_created' || i.status === 'review',
    ).length;
  }, [issues]);

  if (meLoading) {
    return null;
  }

  if (me?.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return (
    <div className="flex flex-col" style={{ minHeight: 'calc(100vh - 3rem - 3rem)' }}>
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-3">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Intake Board</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {totalCount} issue{totalCount !== 1 ? 's' : ''} across all sources
            {slotsInUse > 0 && (
              <span className="ml-2 inline-flex items-center gap-1 text-xs font-medium text-blue-600 dark:text-blue-400">
                <span className="size-1.5 rounded-full bg-blue-500 animate-pulse" />
                {slotsInUse} slot{slotsInUse !== 1 ? 's' : ''} in use
              </span>
            )}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
            <Input
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Filter by title..."
              className="pl-8 h-8 w-44 text-sm"
            />
          </div>
          <select
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value)}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground max-w-48 truncate"
          >
            <option value="all">All sources</option>
            {sources.map(([id, { name, repo }]) => (
              <option key={id} value={String(id)}>
                {name} ({repo})
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Board */}
      {isLoading ? (
        <BoardSkeleton />
      ) : (
        <div className="flex-1 flex flex-col">
          <DragDropContext onDragEnd={onDragEnd}>
            <div className="grid grid-cols-6 gap-2 flex-1">
              {COLUMNS.map((col) => (
                <BoardColumn
                  key={col.id}
                  column={col}
                  issues={columnData.get(col.id) ?? []}
                  onSelect={setSelectedIssue}
                />
              ))}
            </div>
          </DragDropContext>
        </div>
      )}

      {/* Detail dialog */}
      {selectedIssue && (
        <IssueDetailDialog
          issue={selectedIssue}
          onClose={() => setSelectedIssue(null)}
          onStatusChange={handleStatusChange}
        />
      )}
    </div>
  );
}
