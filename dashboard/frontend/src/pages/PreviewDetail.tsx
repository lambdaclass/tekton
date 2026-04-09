import { useEffect, useState, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ExternalLink } from 'lucide-react';
import LogViewer from '@/components/LogViewer';
import { getConfig, listPreviews } from '@/lib/api';
import type { ExtraUrl } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';

export default function PreviewDetail() {
  const { slug } = useParams<{ slug: string }>();
  const navigate = useNavigate();
  const [connected, setConnected] = useState(false);
  const [previewDomain, setPreviewDomain] = useState<string | null>(null);

  useEffect(() => {
    getConfig().then((cfg) => {
      setPreviewDomain(cfg.preview_domain);
    }).catch(() => {});
  }, []);

  const { data: previews } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
  });

  const extraUrls: ExtraUrl[] = previews?.find((p) => p.slug === slug)?.extra_urls ?? [];

  const handleConnectionChange = useCallback((c: boolean) => setConnected(c), []);

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
        <div className="flex items-center gap-2 ml-auto">
          <Button size="sm" asChild>
            <a href={previewUrl} target="_blank" rel="noopener noreferrer">
              Open Preview
              <ExternalLink className="ml-1 size-3" />
            </a>
          </Button>
          {extraUrls.map((eu) => (
            <Button key={eu.label} variant="outline" size="sm" asChild>
              <a href={eu.url} target="_blank" rel="noopener noreferrer">
                {eu.label}
                <ExternalLink className="ml-1 size-3" />
              </a>
            </Button>
          ))}
        </div>
      </div>

      <Card>
        <CardHeader className="py-3">
          <CardTitle className="text-base">Live Logs</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <LogViewer previewSlug={slug} onConnectionChange={handleConnectionChange} />
        </CardContent>
      </Card>
    </div>
  );
}
