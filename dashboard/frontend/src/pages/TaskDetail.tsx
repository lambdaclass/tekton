import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft } from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import { getTask, connectTaskOutput } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import { statusVariant } from '@/lib/status';

export default function TaskDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  const { data: task } = useQuery({
    queryKey: ['task', id],
    queryFn: () => getTask(id!),
    enabled: !!id,
    refetchInterval: 3000,
  });

  useEffect(() => {
    if (!id) return;

    const socket = connectTaskOutput(id);
    socket.addEventListener('open', () => setConnected(true));
    socket.addEventListener('close', () => setConnected(false));
    setWs(socket);

    return () => {
      socket.close();
    };
  }, [id]);

  return (
    <div>
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="sm" onClick={() => navigate('/tasks')}>
          <ChevronLeft className="size-4" />
          Tasks
        </Button>
        <h1 className="text-2xl font-bold font-mono">{id?.slice(0, 8)}</h1>
        {task && <Badge variant={statusVariant(task.status).variant} className={statusVariant(task.status).className}>{task.status}</Badge>}
        <Badge variant={connected ? 'default' : 'outline'}>
          {connected ? 'Live' : 'Disconnected'}
        </Badge>
      </div>

      {task && (
        <Card className="mb-6">
          <CardContent className="py-4">
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground">Repo</span>
                <p>{task.repo}</p>
              </div>
              <div>
                <span className="text-muted-foreground">Base</span>
                <p>{task.base_branch}</p>
              </div>
              {task.branch_name && (
                <div>
                  <span className="text-muted-foreground">Branch</span>
                  <p className="font-mono">{task.branch_name}</p>
                </div>
              )}
              {task.preview_url && (
                <div>
                  <span className="text-muted-foreground">Preview</span>
                  <p>
                    <a
                      href={task.preview_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-blue-400 hover:text-blue-300"
                    >
                      {task.preview_slug}
                    </a>
                  </p>
                </div>
              )}
            </div>
            <Separator className="my-3" />
            <div>
              <span className="text-muted-foreground text-sm">Prompt</span>
              <p className="mt-1 text-sm whitespace-pre-wrap">{task.prompt}</p>
            </div>
            {task.error_message && (
              <>
                <Separator className="my-3" />
                <div>
                  <span className="text-destructive text-sm">Error</span>
                  <p className="mt-1 text-sm text-destructive">{task.error_message}</p>
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader className="py-3">
          <CardTitle className="text-base">Live Logs</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <LogViewer ws={ws} />
        </CardContent>
      </Card>
    </div>
  );
}
