import {
  GitBranch,
  FileEdit,
  Terminal,
  GitCommit,
  Upload,
  ShieldAlert,
  Wrench,
  Play,
  type LucideIcon,
} from 'lucide-react';
import type { TaskAction } from '@/lib/api';
import { timeAgo } from '@/lib/utils';

const ACTION_META: Record<string, { icon: LucideIcon; label: string; color: string }> = {
  clone: { icon: GitBranch, label: 'Cloned repository', color: 'text-blue-400' },
  file_edit: { icon: FileEdit, label: 'Edited file', color: 'text-yellow-400' },
  file_create: { icon: FileEdit, label: 'Created file', color: 'text-green-400' },
  command: { icon: Terminal, label: 'Ran command', color: 'text-cyan-400' },
  commit: { icon: GitCommit, label: 'Created commit', color: 'text-purple-400' },
  push: { icon: Upload, label: 'Pushed to branch', color: 'text-emerald-400' },
  policy_violation: { icon: ShieldAlert, label: 'Policy violation', color: 'text-red-400' },
  tool_use: { icon: Wrench, label: 'Used tool', color: 'text-orange-400' },
};

const DEFAULT_META = { icon: Play, label: 'Action', color: 'text-muted-foreground' };

function actionMeta(actionType: string) {
  return ACTION_META[actionType] ?? DEFAULT_META;
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

  return (
    <div className="relative pl-6">
      {/* Vertical line */}
      <div className="absolute left-[11px] top-2 bottom-2 w-px bg-border" />

      <div className="space-y-1">
        {actions.map((action) => {
          const meta = actionMeta(action.action_type);
          const Icon = meta.icon;
          const summary =
            action.summary ??
            (action.tool_name ? `${meta.label}: ${action.tool_name}` : meta.label);

          return (
            <div key={action.id} className="relative flex items-start gap-3 py-1.5">
              {/* Icon dot */}
              <div
                className={`absolute -left-6 mt-0.5 flex size-[22px] items-center justify-center rounded-full bg-background border border-border ${meta.color}`}
              >
                <Icon className="size-3" />
              </div>

              {/* Content */}
              <div className="min-w-0 flex-1">
                <p className="text-sm leading-tight truncate">{summary}</p>
                <span className="text-xs text-muted-foreground">
                  {timeAgo(action.created_at)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
