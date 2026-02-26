import { useEffect, useRef } from 'react';
import { Link, Outlet, useLocation, useNavigate } from 'react-router-dom';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getMe, logout, listTasks } from '@/lib/api';
import { LayoutDashboard, Container, BrainCircuit, LogOut, Shield } from 'lucide-react';
import { Toaster, toast } from 'sonner';
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';

const ACTIVE_STATUSES = new Set([
  'creating_agent',
  'cloning',
  'running_claude',
  'pushing',
  'creating_preview',
]);

const NAV_ITEMS = [
  { to: '/', label: 'Home', icon: LayoutDashboard },
  { to: '/previews', label: 'Previews', icon: Container },
  { to: '/tasks', label: 'Tasks', icon: BrainCircuit },
];

export default function Layout() {
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const { data: user, isLoading, error } = useQuery({
    queryKey: ['me'],
    queryFn: getMe,
    retry: false,
  });

  // Poll all tasks for running count + toast notifications
  const { data: allTasksData } = useQuery({
    queryKey: ['tasks', { per_page: 200 }],
    queryFn: () => listTasks({ per_page: 200 }),
    refetchInterval: 5000,
  });

  const allTasks = allTasksData?.tasks ?? [];
  const runningCount = allTasks.filter((t) => ACTIVE_STATUSES.has(t.status)).length;

  // Dynamic document title
  useEffect(() => {
    document.title = runningCount > 0 ? `Tekton (${runningCount} running)` : 'Tekton';
  }, [runningCount]);

  // Toast notifications on status transitions
  const prevStatuses = useRef<Map<string, string> | null>(null);
  useEffect(() => {
    if (!allTasks.length) return;

    const currentMap = new Map(allTasks.map((t) => [t.id, t.status]));

    if (prevStatuses.current !== null) {
      for (const [id, status] of currentMap) {
        const prev = prevStatuses.current.get(id);
        if (!prev) continue;
        if (prev !== status) {
          const task = allTasks.find((t) => t.id === id);
          const label = task?.name || id.slice(0, 8);
          if (status === 'completed') {
            toast.success(`Task ${label} completed`);
          } else if (status === 'failed') {
            toast.error(`Task ${label} failed`);
          }
        }
      }
    }

    prevStatuses.current = currentMap;
  }, [allTasks]);

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="text-muted-foreground">Loading...</div>
      </div>
    );
  }

  if (error || !user) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="text-center">
          <h1 className="text-2xl font-bold mb-4">Preview Dashboard</h1>
          <p className="text-muted-foreground mb-6">
            Sign in with your GitHub account
          </p>
          <Button asChild size="lg">
            <a href="/api/auth/login">Sign in with GitHub</a>
          </Button>
        </div>
      </div>
    );
  }

  const handleLogout = async () => {
    await logout();
    queryClient.clear();
    navigate('/');
    window.location.reload();
  };

  const initials = user.login
    .slice(0, 2)
    .toUpperCase();

  const isActive = (path: string) => {
    if (path === '/') return location.pathname === '/';
    return location.pathname.startsWith(path);
  };

  return (
    <SidebarProvider defaultOpen={true}>
      <Sidebar collapsible="icon">
        <SidebarHeader className="p-3">
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton size="lg" asChild tooltip="Dashboard">
                <Link to="/">
                  <div className="flex aspect-square size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
                    <LayoutDashboard className="size-4" />
                  </div>
                  <span className="font-semibold">Dashboard</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu>
                {NAV_ITEMS.map((item) => (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton
                      asChild
                      isActive={isActive(item.to)}
                      tooltip={item.label}
                    >
                      <Link to={item.to}>
                        <item.icon />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                    {item.to === '/tasks' && runningCount > 0 && (
                      <SidebarMenuBadge>{runningCount}</SidebarMenuBadge>
                    )}
                  </SidebarMenuItem>
                ))}
                {user.role === 'admin' && (
                  <SidebarMenuItem>
                    <SidebarMenuButton
                      asChild
                      isActive={isActive('/admin')}
                      tooltip="Admin"
                    >
                      <Link to="/admin">
                        <Shield />
                        <span>Admin</span>
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                )}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
        <SidebarFooter>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip={user.login} className="cursor-default">
                <Avatar className="size-5">
                  <AvatarFallback className="text-[10px]">{initials}</AvatarFallback>
                </Avatar>
                <span className="truncate text-xs">{user.login}</span>
                <Badge variant="outline" className="ml-auto text-[10px] px-1.5 py-0">
                  {user.role}
                </Badge>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip="Logout" onClick={handleLogout}>
                <LogOut />
                <span>Logout</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset>
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="mr-2 h-4" />
          <span className="text-sm text-muted-foreground">
            {NAV_ITEMS.find((n) => isActive(n.to))?.label ?? (isActive('/admin') ? 'Admin' : '')}
          </span>
        </header>
        <main className="flex-1 p-6">
          <Outlet />
        </main>
      </SidebarInset>
      <Toaster position="bottom-right" richColors closeButton />
    </SidebarProvider>
  );
}
