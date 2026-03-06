import { useState, useMemo, useRef, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import {
  DragDropContext,
  Droppable,
  Draggable,
  type DropResult,
} from '@hello-pangea/dnd';
import {
  listAllIntakeIssues,
  updateIntakeIssueStatus,
  type IntakeIssueWithMeta,
} from '@/lib/api';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { cn, timeAgo } from '@/lib/utils';
import {
  Search,
  ExternalLink,
  AlertTriangle,
  Inbox,
  Clock,
  Play,
  Eye,
  CheckCircle2,
  XCircle,
} from 'lucide-react';

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
    <div className="flex gap-3 pb-4">
      {COLUMNS.map((col) => (
        <div
          key={col.id}
          className="flex-1 min-w-0 rounded-xl bg-muted/40 border border-border/50 p-3"
        >
          <div className="flex items-center gap-2 mb-3 px-1">
            <Skeleton className="h-4 w-24 rounded" />
            <Skeleton className="h-5 w-6 rounded-full" />
          </div>
          <div className="space-y-2.5">
            {Array.from({ length: col.id === 'backlog' ? 3 : 2 }).map(
              (_, i) => (
                <div
                  key={i}
                  className="rounded-lg bg-background border border-border/50 p-3 space-y-2"
                >
                  <Skeleton className="h-4 w-4/5 rounded" />
                  <Skeleton className="h-3 w-1/3 rounded" />
                  <div className="flex gap-1.5">
                    <Skeleton className="h-4 w-12 rounded-full" />
                    <Skeleton className="h-4 w-14 rounded-full" />
                  </div>
                  <Skeleton className="h-3 w-1/4 rounded" />
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
// Issue card
// ---------------------------------------------------------------------------

interface IssueCardProps {
  issue: IntakeIssueWithMeta;
  index: number;
}

function IssueCard({ issue, index }: IssueCardProps) {
  return (
    <Draggable draggableId={String(issue.id)} index={index}>
      {(provided, snapshot) => (
        <div
          ref={provided.innerRef}
          {...provided.draggableProps}
          {...provided.dragHandleProps}
          className={cn(
            'rounded-lg bg-background border border-border/60 p-3 transition-shadow select-none',
            'hover:border-border hover:shadow-sm',
            snapshot.isDragging &&
              'shadow-lg ring-2 ring-primary/20 rotate-[1.5deg] scale-[1.02]',
          )}
        >
          {/* Title */}
          <p className="text-sm font-medium leading-snug line-clamp-2 mb-1.5">
            {issue.external_title}
          </p>

          {/* Source + external link */}
          <div className="flex items-center gap-1.5 mb-2">
            <span className="text-xs text-muted-foreground truncate">
              {issue.source_name}
            </span>
            {issue.external_url && (
              <a
                href={issue.external_url}
                target="_blank"
                rel="noopener noreferrer"
                onClick={(e) => e.stopPropagation()}
                className="text-muted-foreground hover:text-foreground transition-colors"
                title="Open external issue"
              >
                <ExternalLink className="size-3" />
              </a>
            )}
          </div>

          {/* Labels */}
          {issue.external_labels.length > 0 && (
            <div className="flex flex-wrap gap-1 mb-2">
              {issue.external_labels.map((lbl) => (
                <span
                  key={lbl}
                  className={cn(
                    'inline-flex items-center rounded-full border px-1.5 py-0 text-[10px] font-medium leading-4',
                    labelColor(lbl),
                  )}
                >
                  {lbl}
                </span>
              ))}
            </div>
          )}

          {/* Footer: task link + time */}
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <div className="flex items-center gap-2 min-w-0">
              {issue.task_id && (
                <Link
                  to={`/tasks/${issue.task_id}`}
                  onClick={(e) => e.stopPropagation()}
                  className="font-mono text-blue-600 dark:text-blue-400 hover:underline truncate"
                  title={`Task ${issue.task_id}`}
                >
                  {issue.task_id.slice(0, 8)}
                </Link>
              )}
              {issue.error_message && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="text-red-500 dark:text-red-400 flex items-center gap-0.5 shrink-0">
                      <AlertTriangle className="size-3" />
                    </span>
                  </TooltipTrigger>
                  <TooltipContent
                    side="bottom"
                    className="max-w-xs text-xs"
                  >
                    {issue.error_message}
                  </TooltipContent>
                </Tooltip>
              )}
            </div>
            <span
              className="shrink-0"
              title={new Date(issue.updated_at).toLocaleString()}
            >
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
}

function BoardColumn({ column, issues }: BoardColumnProps) {
  const listRef = useRef<HTMLDivElement>(null);

  // Scroll to top when new issues appear at position 0
  useEffect(() => {
    listRef.current?.scrollTo({ top: 0, behavior: 'smooth' });
  }, [issues.length]);

  return (
    <div className="flex-1 min-w-0 flex flex-col rounded-xl bg-muted/40 border border-border/50">
      {/* Column header */}
      <div className="flex items-center gap-2 px-3 pt-3 pb-2">
        <span className={cn('size-2 rounded-full', column.dotColor)} />
        <span
          className={cn(
            'text-sm font-semibold',
            column.headerColor,
          )}
        >
          {column.label}
        </span>
        <Badge
          variant="secondary"
          className="ml-auto text-[10px] px-1.5 py-0 h-5 tabular-nums"
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
              'flex-1 overflow-y-auto px-2 pb-2 space-y-2 min-h-[120px] transition-colors rounded-b-xl',
              snapshot.isDraggingOver && 'bg-primary/5',
            )}
          >
            {issues.map((issue, idx) => (
              <IssueCard key={issue.id} issue={issue} index={idx} />
            ))}
            {provided.placeholder}
            {issues.length === 0 && !snapshot.isDraggingOver && (
              <div className="flex items-center justify-center h-20 text-xs text-muted-foreground/50">
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

  // Data fetching
  const { data: issues, isLoading } = useQuery({
    queryKey: ['intake-issues-all'],
    queryFn: listAllIntakeIssues,
    refetchInterval: 5000,
  });

  // Filters
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [sourceFilter, setSourceFilter] = useState('all');

  // Debounce search
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      setSearch(searchInput);
    }, 300);
    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [searchInput]);

  // Unique sources for dropdown
  const sources = useMemo(() => {
    if (!issues) return [];
    const map = new Map<number, string>();
    for (const issue of issues) {
      map.set(issue.source_id, issue.source_name);
    }
    return Array.from(map.entries()).sort((a, b) => a[1].localeCompare(b[1]));
  }, [issues]);

  // Filtered issues
  const filtered = useMemo(() => {
    if (!issues) return [];
    return issues.filter((issue) => {
      if (sourceFilter !== 'all' && String(issue.source_id) !== sourceFilter)
        return false;
      if (
        search &&
        !issue.external_title.toLowerCase().includes(search.toLowerCase())
      )
        return false;
      return true;
    });
  }, [issues, sourceFilter, search]);

  // Group by status
  const columnData = useMemo(() => {
    const map = new Map<string, IntakeIssueWithMeta[]>();
    for (const col of COLUMNS) {
      map.set(col.id, []);
    }
    for (const issue of filtered) {
      const bucket = map.get(issue.status);
      if (bucket) {
        bucket.push(issue);
      } else {
        // Unknown statuses go to backlog
        map.get('backlog')!.push(issue);
      }
    }
    // Sort each column by updated_at descending
    for (const arr of map.values()) {
      arr.sort(
        (a, b) =>
          new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
      );
    }
    return map;
  }, [filtered]);

  // Drag-and-drop mutation
  const statusMutation = useMutation({
    mutationFn: ({ id, status }: { id: number; status: string }) =>
      updateIntakeIssueStatus(id, status),
    onMutate: async ({ id, status }) => {
      await queryClient.cancelQueries({ queryKey: ['intake-issues-all'] });
      const prev = queryClient.getQueryData<IntakeIssueWithMeta[]>([
        'intake-issues-all',
      ]);
      queryClient.setQueryData<IntakeIssueWithMeta[]>(
        ['intake-issues-all'],
        (old) =>
          old?.map((issue) =>
            issue.id === id ? { ...issue, status } : issue,
          ),
      );
      return { prev };
    },
    onError: (_err, _vars, context) => {
      if (context?.prev) {
        queryClient.setQueryData(['intake-issues-all'], context.prev);
      }
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
    statusMutation.mutate({ id: issueId, status: newStatus });
  };

  const totalCount = issues?.length ?? 0;

  return (
    <div className="flex flex-col" style={{ minHeight: 'calc(100vh - 3rem - 3rem)' }}>
      {/* Page header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Intake Board</h1>
          <p className="text-sm text-muted-foreground mt-0.5">
            {totalCount} issue{totalCount !== 1 ? 's' : ''} across all sources
          </p>
        </div>

        {/* Filters */}
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
            <Input
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Filter by title..."
              className="pl-8 h-8 w-52 text-sm"
            />
          </div>
          <select
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value)}
            className="h-8 rounded-md border border-input bg-background px-2.5 text-sm text-foreground"
          >
            <option value="all">All sources</option>
            {sources.map(([id, name]) => (
              <option key={id} value={String(id)}>
                {name}
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
            <div className="flex gap-3 flex-1 pb-2">
              {COLUMNS.map((col) => (
                <BoardColumn
                  key={col.id}
                  column={col}
                  issues={columnData.get(col.id) ?? []}
                />
              ))}
            </div>
          </DragDropContext>
        </div>
      )}
    </div>
  );
}
