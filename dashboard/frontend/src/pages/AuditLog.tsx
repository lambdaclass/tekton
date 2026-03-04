import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Navigate, Link } from 'react-router-dom';
import { getMe, getAuditLog } from '@/lib/api';
import type { AuditLogEntry } from '@/lib/api';
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
import { ScrollText, ChevronLeft, ChevronRight, ChevronDown, ChevronUp } from 'lucide-react';

const EVENT_TYPES = [
  '',
  'task.created',
  'task.completed',
  'task.failed',
  'task.reopened',
  'task.message_sent',
  'task.renamed',
  'task.pr_created',
  'task.pr_linked',
  'preview.created',
  'preview.destroyed',
  'preview.updated',
  'admin.org_policy_create',
  'admin.org_policy_update',
  'admin.org_policy_delete',
  'admin.budget_create',
  'admin.budget_update',
  'admin.budget_delete',
  'admin.user_repos_changed',
  'admin.ai_settings_update',
  'admin.ai_settings_delete',
  'user.login',
  'user.role_changed',
  'budget.created',
  'budget.deleted',
  'budget.alert',
  'secret.created',
  'secret.deleted',
  'policy.created',
  'policy.deleted',
] as const;

const EVENT_COLORS: Record<string, string> = {
  'task.created': 'bg-blue-500/15 text-blue-700 dark:text-blue-400',
  'task.completed': 'bg-green-500/15 text-green-700 dark:text-green-400',
  'task.failed': 'bg-red-500/15 text-red-700 dark:text-red-400',
  'task.reopened': 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-400',
  'task.message_sent': 'bg-blue-500/15 text-blue-700 dark:text-blue-400',
  'task.renamed': 'bg-blue-500/15 text-blue-700 dark:text-blue-400',
  'task.pr_created': 'bg-green-500/15 text-green-700 dark:text-green-400',
  'task.pr_linked': 'bg-green-500/15 text-green-700 dark:text-green-400',
  'preview.created': 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-400',
  'preview.destroyed': 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-400',
  'preview.updated': 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-400',
  'admin.org_policy_create': 'bg-indigo-500/15 text-indigo-700 dark:text-indigo-400',
  'admin.org_policy_update': 'bg-indigo-500/15 text-indigo-700 dark:text-indigo-400',
  'admin.org_policy_delete': 'bg-indigo-500/15 text-indigo-700 dark:text-indigo-400',
  'admin.budget_create': 'bg-purple-500/15 text-purple-700 dark:text-purple-400',
  'admin.budget_update': 'bg-purple-500/15 text-purple-700 dark:text-purple-400',
  'admin.budget_delete': 'bg-purple-500/15 text-purple-700 dark:text-purple-400',
  'admin.user_repos_changed': 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-400',
  'admin.ai_settings_update': 'bg-teal-500/15 text-teal-700 dark:text-teal-400',
  'admin.ai_settings_delete': 'bg-teal-500/15 text-teal-700 dark:text-teal-400',
  'user.login': 'bg-gray-500/15 text-gray-700 dark:text-gray-400',
  'user.role_changed': 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-400',
  'budget.created': 'bg-purple-500/15 text-purple-700 dark:text-purple-400',
  'budget.deleted': 'bg-purple-500/15 text-purple-700 dark:text-purple-400',
  'budget.alert': 'bg-orange-500/15 text-orange-700 dark:text-orange-400',
  'secret.created': 'bg-teal-500/15 text-teal-700 dark:text-teal-400',
  'secret.deleted': 'bg-teal-500/15 text-teal-700 dark:text-teal-400',
  'policy.created': 'bg-indigo-500/15 text-indigo-700 dark:text-indigo-400',
  'policy.deleted': 'bg-indigo-500/15 text-indigo-700 dark:text-indigo-400',
};

const PER_PAGE = 25;

export default function AuditLog() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const [page, setPage] = useState(1);
  const [filters, setFilters] = useState({
    event_type: '',
    actor: '',
    target: '',
    start_date: '',
    end_date: '',
  });
  const [expandedId, setExpandedId] = useState<number | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ['audit-log', filters, page],
    queryFn: () =>
      getAuditLog({
        event_type: filters.event_type || undefined,
        actor: filters.actor || undefined,
        target: filters.target || undefined,
        start_date: filters.start_date || undefined,
        end_date: filters.end_date || undefined,
        page,
        per_page: PER_PAGE,
      }),
  });

  if (me && me.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  const totalPages = data ? Math.ceil(data.total / PER_PAGE) : 0;

  const handleFilterChange = (key: string, value: string) => {
    setFilters((f) => ({ ...f, [key]: value }));
    setPage(1);
  };

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Audit Log</h1>

      {/* Filters */}
      <Card>
        <CardContent className="pt-6">
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Event Type</Label>
              <Select
                value={filters.event_type}
                onValueChange={(v) => handleFilterChange('event_type', v === 'all' ? '' : v)}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="All events" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All events</SelectItem>
                  {EVENT_TYPES.filter(Boolean).map((t) => (
                    <SelectItem key={t} value={t}>
                      {t}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Actor</Label>
              <Input
                placeholder="Filter by actor..."
                value={filters.actor}
                onChange={(e) => handleFilterChange('actor', e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Target</Label>
              <Input
                placeholder="Filter by target..."
                value={filters.target}
                onChange={(e) => handleFilterChange('target', e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Start Date</Label>
              <Input
                type="date"
                value={filters.start_date}
                onChange={(e) => handleFilterChange('start_date', e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">End Date</Label>
              <Input
                type="date"
                value={filters.end_date}
                onChange={(e) => handleFilterChange('end_date', e.target.value)}
              />
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Table */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <ScrollText className="size-5" />
              Events
              {data && (
                <span className="text-sm font-normal text-muted-foreground">
                  ({data.total} total)
                </span>
              )}
            </CardTitle>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <p className="text-muted-foreground text-sm">Loading audit log...</p>
          ) : !data?.entries?.length ? (
            <p className="text-muted-foreground text-sm">No events found.</p>
          ) : (
            <>
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b text-left text-xs uppercase tracking-wider text-muted-foreground">
                      <th className="py-2 pr-4 font-medium">Timestamp</th>
                      <th className="py-2 pr-4 font-medium">Event Type</th>
                      <th className="py-2 pr-4 font-medium">Actor</th>
                      <th className="py-2 pr-4 font-medium">Target</th>
                      <th className="py-2 font-medium">Detail</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.entries.map((entry: AuditLogEntry) => (
                      <AuditRow
                        key={entry.id}
                        entry={entry}
                        isExpanded={expandedId === entry.id}
                        onToggle={() =>
                          setExpandedId(expandedId === entry.id ? null : entry.id)
                        }
                      />
                    ))}
                  </tbody>
                </table>
              </div>

              {/* Pagination */}
              {totalPages > 1 && (
                <div className="flex items-center justify-between pt-4">
                  <p className="text-sm text-muted-foreground">
                    Page {page} of {totalPages}
                  </p>
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setPage((p) => Math.max(1, p - 1))}
                      disabled={page <= 1}
                    >
                      <ChevronLeft className="size-4" />
                      Prev
                    </Button>
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
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function AuditRow({
  entry,
  isExpanded,
  onToggle,
}: {
  entry: AuditLogEntry;
  isExpanded: boolean;
  onToggle: () => void;
}) {
  const colorClass = EVENT_COLORS[entry.event_type] || 'bg-gray-500/15 text-gray-700 dark:text-gray-400';

  return (
    <>
      <tr className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
        <td className="py-2 pr-4 whitespace-nowrap">
          {new Date(entry.created_at).toLocaleString()}
        </td>
        <td className="py-2 pr-4">
          <Badge variant="secondary" className={colorClass}>
            {entry.event_type}
          </Badge>
        </td>
        <td className="py-2 pr-4 font-mono">{entry.actor}</td>
        <td className="py-2 pr-4 font-mono">
          {entry.target && entry.event_type.startsWith('task.') ? (
            <Link to={`/tasks/${entry.target}`} className="text-blue-500 hover:underline">
              {entry.target}
            </Link>
          ) : (
            entry.target ?? '-'
          )}
        </td>
        <td className="py-2">
          {entry.detail ? (
            <Button variant="ghost" size="sm" onClick={onToggle} className="h-7 px-2">
              {isExpanded ? (
                <ChevronUp className="size-3 mr-1" />
              ) : (
                <ChevronDown className="size-3 mr-1" />
              )}
              {isExpanded ? 'Hide' : 'Show'}
            </Button>
          ) : (
            <span className="text-muted-foreground">-</span>
          )}
        </td>
      </tr>
      {isExpanded && entry.detail && (
        <tr className="border-b border-border/50 hover:bg-secondary/40 transition-colors duration-100">
          <td colSpan={5} className="py-2 px-4">
            <pre className="bg-muted rounded-md p-3 text-xs overflow-x-auto whitespace-pre-wrap">
              {JSON.stringify(entry.detail, null, 2)}
            </pre>
          </td>
        </tr>
      )}
    </>
  );
}
