import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ChevronLeft, ExternalLink } from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import { connectPreviewLogs, getConfig } from '@/lib/api';
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
    getConfig().then((cfg) => {
      setPreviewDomain(cfg.preview_domain);
    }).catch(() => {});
  }, []);

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
