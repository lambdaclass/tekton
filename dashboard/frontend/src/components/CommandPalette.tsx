import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Command as CommandPrimitive } from 'cmdk';
import { listTasks, getMe, type Task } from '@/lib/api';
import { statusVariant } from '@/lib/status';
import { timeAgo } from '@/lib/utils';
import {
  LayoutDashboard,
  BrainCircuit,
  Container,
  SlidersHorizontal,
  Shield,
  DollarSign,
  ScrollText,
  Plus,
  Search,
} from 'lucide-react';

export default function CommandPalette() {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const { data: user } = useQuery({ queryKey: ['me'], queryFn: getMe });

  const { data: tasksData } = useQuery({
    queryKey: ['tasks', { per_page: 50 }],
    queryFn: () => listTasks({ per_page: 50 }),
    refetchInterval: 10000,
  });

  const tasks = tasksData?.tasks ?? [];

  // Cmd+K / Ctrl+K to open
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'k' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((prev) => !prev);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, []);

  const go = (path: string) => {
    setOpen(false);
    navigate(path);
  };

  const pages = [
    { label: 'Home', icon: LayoutDashboard, path: '/' },
    { label: 'Tasks', icon: BrainCircuit, path: '/tasks' },
    { label: 'Previews', icon: Container, path: '/previews' },
    { label: 'Settings', icon: SlidersHorizontal, path: '/settings' },
  ];

  const adminPages = [
    { label: 'Admin', icon: Shield, path: '/admin' },
    { label: 'Cost Dashboard', icon: DollarSign, path: '/cost' },
    { label: 'Audit Log', icon: ScrollText, path: '/audit' },
  ];

  const taskLabel = (t: Task) => t.name || t.prompt.slice(0, 60);

  return (
    <CommandPrimitive.Dialog
      open={open}
      onOpenChange={setOpen}
      label="Command palette"
      overlayClassName="fixed inset-0 z-50 bg-black/40 backdrop-blur-sm"
      contentClassName="fixed top-[20%] left-1/2 z-50 w-full max-w-lg -translate-x-1/2 rounded-lg border border-border bg-card/90 backdrop-blur-xl shadow-2xl"
    >
      <div className="flex items-center gap-2 border-b border-border px-3">
        <Search className="size-4 shrink-0 text-muted-foreground" />
        <CommandPrimitive.Input
          placeholder="Search tasks, pages..."
          className="flex h-11 w-full bg-transparent text-sm text-foreground placeholder:text-muted-foreground outline-none"
        />
      </div>

      <CommandPrimitive.List className="max-h-72 overflow-y-auto p-2">
        <CommandPrimitive.Empty className="py-6 text-center text-sm text-muted-foreground">
          No results found.
        </CommandPrimitive.Empty>

        <CommandPrimitive.Group
          heading="Pages"
          className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground"
        >
          {pages.map((p) => (
            <CommandPrimitive.Item
              key={p.path}
              value={p.label}
              onSelect={() => go(p.path)}
              className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-foreground aria-selected:bg-primary/10"
            >
              <p.icon className="size-4 text-muted-foreground" />
              {p.label}
            </CommandPrimitive.Item>
          ))}
          {user?.role === 'admin' &&
            adminPages.map((p) => (
              <CommandPrimitive.Item
                key={p.path}
                value={p.label}
                onSelect={() => go(p.path)}
                className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-foreground aria-selected:bg-primary/10"
              >
                <p.icon className="size-4 text-muted-foreground" />
                {p.label}
              </CommandPrimitive.Item>
            ))}
        </CommandPrimitive.Group>

        <CommandPrimitive.Separator className="my-1 h-px bg-border" />

        <CommandPrimitive.Group
          heading="Actions"
          className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground"
        >
          <CommandPrimitive.Item
            value="Create new task"
            onSelect={() => go('/tasks?create=1')}
            className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-foreground aria-selected:bg-primary/10"
          >
            <Plus className="size-4 text-muted-foreground" />
            Create new task
          </CommandPrimitive.Item>
        </CommandPrimitive.Group>

        {tasks.length > 0 && (
          <>
            <CommandPrimitive.Separator className="my-1 h-px bg-border" />
            <CommandPrimitive.Group
              heading="Tasks"
              className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground"
            >
              {tasks.map((t) => {
                const sv = statusVariant(t.status);
                const StatusIcon = sv.icon;
                return (
                  <CommandPrimitive.Item
                    key={t.id}
                    value={`${t.id} ${taskLabel(t)} ${t.repo}`}
                    keywords={[t.status, t.repo, t.name ?? '', t.prompt]}
                    onSelect={() => go(`/tasks/${t.id}`)}
                    className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm text-foreground aria-selected:bg-primary/10"
                  >
                    {StatusIcon && (
                      <StatusIcon
                        className={`size-3.5 shrink-0 ${sv.spin ? 'animate-spin' : ''} ${
                          sv.className?.includes('text-') ? '' : 'text-muted-foreground'
                        }`}
                      />
                    )}
                    <span className="truncate flex-1">{taskLabel(t)}</span>
                    <span className="shrink-0 text-xs text-muted-foreground font-mono">
                      {t.id.slice(0, 8)}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {timeAgo(t.created_at)}
                    </span>
                  </CommandPrimitive.Item>
                );
              })}
            </CommandPrimitive.Group>
          </>
        )}
      </CommandPrimitive.List>
    </CommandPrimitive.Dialog>
  );
}
