import { useEffect, useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import LogViewer from '../components/LogViewer';
import { connectPreviewLogs } from '../lib/api';

export default function PreviewDetail() {
  const { slug } = useParams<{ slug: string }>();
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

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

  const previewUrl = slug ? `https://${slug}.hipermegared.link` : '';

  return (
    <div>
      <div className="flex items-center gap-4 mb-6">
        <Link to="/previews" className="text-gray-400 hover:text-gray-100 transition-colors">
          &larr; Previews
        </Link>
        <h1 className="text-2xl font-bold font-mono">{slug}</h1>
        <span
          className={`text-xs px-2 py-0.5 rounded ${
            connected ? 'bg-green-900 text-green-300' : 'bg-gray-800 text-gray-400'
          }`}
        >
          {connected ? 'Live' : 'Disconnected'}
        </span>
        <a
          href={previewUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="ml-auto px-4 py-1.5 text-sm bg-blue-600 hover:bg-blue-700 rounded transition-colors"
        >
          Open Preview
        </a>
      </div>
      <LogViewer ws={ws} />
    </div>
  );
}
