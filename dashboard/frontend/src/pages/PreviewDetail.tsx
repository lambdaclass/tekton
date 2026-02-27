import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ExternalLink } from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import { connectPreviewLogs, getConfig, listPreviews } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';

export default function PreviewDetail() {
  const { slug } = useParams<{ slug: string }>();
  const navigate = useNavigate();
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  const [previewDomain, setPreviewDomain] = useState<string | null>(null);

  useEffect(() => {
    getConfig().then((cfg) => setPreviewDomain(cfg.preview_domain)).catch(() => {});
  }, []);

  const { data: previews } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
  });
  const preview = previews?.find((p) => p.slug === slug);

  useEffect(() => {
    if (!slug) return;

    const socket = connectPreviewLogs(slug);
    socket.addEventListener('open', () => setConnected(true));
    socket.addEventListener('close', () => setConnected(false));
    setWs(socket);

    return () => {
      socket.close();
    };
  }, [slug]);

  const previewUrl = slug && previewDomain ? `https://${slug}.${previewDomain}` : '';

  return (
    <div>
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="sm" onClick={() => navigate('/previews')}>
          <ChevronLeft className="size-4" />
          Previews
        </Button>
        <h1 className="text-2xl font-bold font-mono">{slug}</h1>
        <Badge variant={connected ? 'default' : 'outline'}>
          {connected ? 'Live' : 'Disconnected'}
        </Badge>
        <Button size="sm" className="ml-auto" asChild>
          <a href={previewUrl} target="_blank" rel="noopener noreferrer">
            Open Preview
            <ExternalLink className="ml-1 size-3" />
          </a>
        </Button>
      </div>

      {preview?.ssh_port && previewDomain && (
        <div className="mb-6 px-3 py-2 bg-muted rounded-md">
          <span className="text-xs text-muted-foreground">SSH: </span>
          <code className="text-xs font-mono">ssh root@{previewDomain} -p {preview.ssh_port}</code>
        </div>
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
