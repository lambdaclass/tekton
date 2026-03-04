import { useState } from 'react';
import {
  GitBranch,
  FileEdit,
  Terminal,
  GitCommit,
  Upload,
  ShieldAlert,
  Wrench,
  Play,
  ChevronRight,
  type LucideIcon,
} from 'lucide-react';
import type { TaskAction } from '@/lib/api';
import { timeAgo } from '@/lib/utils';

const ACTION_META: Record<string, { icon: LucideIcon; label: string; color: string; important?: boolean }> = {
  clone:            { icon: GitBranch,  label: 'Cloned repository', color: 'text-blue-700 dark:text-blue-400',    important: true },
  file_edit:        { icon: FileEdit,   label: 'Edited file',       color: 'text-yellow-700 dark:text-yellow-400' },
  file_create:      { icon: FileEdit,   label: 'Created file',      color: 'text-green-700 dark:text-green-400' },
  command:          { icon: Terminal,    label: 'Ran command',       color: 'text-cyan-700 dark:text-cyan-400' },
  commit:           { icon: GitCommit,  label: 'Created commit',    color: 'text-purple-700 dark:text-purple-400', important: true },
  push:             { icon: Upload,     label: 'Pushed to branch',  color: 'text-emerald-700 dark:text-emerald-400', important: true },
  policy_violation: { icon: ShieldAlert, label: 'Policy violation', color: 'text-red-700 dark:text-red-400',       important: true },
  tool_use:         { icon: Wrench,     label: 'Used tool',         color: 'text-muted-foreground' },
};

const DEFAULT_META = { icon: Play, label: 'Action', color: 'text-muted-foreground' };

function actionMeta(actionType: string) {
  return ACTION_META[actionType] ?? DEFAULT_META;
}

/** Shorten long paths to just the filename (or last 2 segments). */
function shortenPath(s: string): string {
  const match = s.match(/\/([^/]+\/[^/]+)$/);
  return match ? match[1] : s;
}

/** Build a concise summary for a single action. */
function actionSummary(action: TaskAction): string {
  if (action.summary) {
    // Shorten "Reading /very/long/path/to/file.json" style summaries
    return action.summary.replace(/(?:\/[\w.-]+){3,}/g, (p) => '…/' + shortenPath(p));
  }
  const meta = actionMeta(action.action_type);
  return action.tool_name ? `${meta.label}: ${action.tool_name}` : meta.label;
}

/** Group consecutive actions with the same action_type + tool_name. */
interface ActionGroup {
  key: string;
  actions: TaskAction[];
  actionType: string;
  toolName: string | null;
}

function groupActions(actions: TaskAction[]): ActionGroup[] {
  const groups: ActionGroup[] = [];
  for (const action of actions) {
    const last = groups[groups.length - 1];
    const groupKey = `${action.action_type}:${action.tool_name ?? ''}`;
    if (last && last.key === groupKey) {
      last.actions.push(action);
    } else {
      groups.push({
        key: groupKey,
        actions: [action],
        actionType: action.action_type,
        toolName: action.tool_name,
      });
    }
  }
  return groups;
}

interface ActivityTimelineProps {
  actions: TaskAction[] | undefined;
}

export default function ActivityTimeline({ actions }: ActivityTimelineProps) {
  if (!actions || actions.length === 0) {
    return (
      <p className="text-sm text-muted-foreground text-center py-8">
        No activity recorded yet.
      </p>
    );
  }

  const groups = groupActions(actions);

  return (
    <div className="relative pl-6">
      {/* Vertical line */}
      <div className="absolute left-[11px] top-2 bottom-2 w-px bg-border" />

      <div className="space-y-0.5">
        {groups.map((group, gi) => (
          <ActionGroupRow key={gi} group={group} />
        ))}
      </div>
    </div>
  );
}

function ActionGroupRow({ group }: { group: ActionGroup }) {
  const [expanded, setExpanded] = useState(false);
  const meta = actionMeta(group.actionType);
  const Icon = meta.icon;
  const isGroup = group.actions.length > 1;
  const isImportant = meta.important;

  // For single actions or important ones, render directly
  if (!isGroup) {
    const action = group.actions[0];
    return (
      <SingleAction action={action} meta={meta} Icon={Icon} important={isImportant} />
    );
  }

  // Grouped: show collapsed summary that expands
  const first = group.actions[0];
  const last = group.actions[group.actions.length - 1];
  const toolLabel = first.tool_name || meta.label;
  const groupLabel = `${toolLabel} × ${group.actions.length}`;

  return (
    <div>
      <button
        onClick={() => setExpanded(!expanded)}
        className="relative flex items-start gap-3 py-1.5 px-2 w-full text-left hover:bg-secondary/40 rounded-md transition-colors duration-100 group"
      >
        {/* Icon dot */}
        <div
          className={`absolute -left-6 mt-0.5 flex size-[24px] items-center justify-center rounded-full bg-secondary border border-border ${meta.color}`}
        >
          <Icon className="size-3" />
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1 flex items-center gap-2">
          <ChevronRight className={`size-3 text-muted-foreground shrink-0 transition-transform duration-150 ${expanded ? 'rotate-90' : ''}`} />
          <span className="text-sm text-muted-foreground truncate">{groupLabel}</span>
          <span className="text-xs text-muted-foreground/60 ml-auto shrink-0">
            {timeAgo(last.created_at)}
          </span>
        </div>
      </button>

      {/* Expanded children */}
      {expanded && (
        <div className="ml-4 border-l border-border/50 pl-2 mb-1">
          {group.actions.map((action) => (
            <div key={action.id} className="py-1 px-2 text-xs text-muted-foreground truncate hover:bg-secondary/30 rounded transition-colors">
              {actionSummary(action)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SingleAction({
  action,
  meta,
  Icon,
  important,
}: {
  action: TaskAction;
  meta: { icon: LucideIcon; label: string; color: string };
  Icon: LucideIcon;
  important?: boolean;
}) {
  return (
    <div className={`relative flex items-start gap-3 py-1.5 hover:bg-secondary/40 rounded-md transition-colors duration-150 px-2 ${important ? '' : 'opacity-80'}`}>
      {/* Icon dot */}
      <div
        className={`absolute -left-6 mt-0.5 flex size-[24px] items-center justify-center rounded-full bg-secondary border border-border ${meta.color}`}
      >
        <Icon className="size-3" />
      </div>

      {/* Content */}
      <div className="min-w-0 flex-1">
        <p className={`text-sm leading-tight truncate ${important ? 'font-medium' : ''}`}>
          {actionSummary(action)}
        </p>
        <span className="text-xs text-muted-foreground">
          {timeAgo(action.created_at)}
        </span>
      </div>
    </div>
  );
}
