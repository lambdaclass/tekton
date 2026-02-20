import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

interface LogViewerProps {
  ws: WebSocket | null;
}

export default function LogViewer({ ws }: LogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      convertEol: true,
      fontSize: 13,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#1a1b26',
        foreground: '#c0caf5',
        cursor: '#c0caf5',
      },
      scrollback: 10000,
      disableStdin: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    const onResize = () => fit.fit();
    window.addEventListener('resize', onResize);

    return () => {
      window.removeEventListener('resize', onResize);
      term.dispose();
    };
  }, []);

  useEffect(() => {
    if (!ws || !termRef.current) return;

    const term = termRef.current;

    const onMessage = (ev: MessageEvent) => {
      term.writeln(ev.data);
    };
    const onClose = () => {
      term.writeln('\r\n\x1b[90m--- Connection closed ---\x1b[0m');
    };

    ws.addEventListener('message', onMessage);
    ws.addEventListener('close', onClose);

    return () => {
      ws.removeEventListener('message', onMessage);
      ws.removeEventListener('close', onClose);
    };
  }, [ws]);

  return (
    <div
      ref={containerRef}
      className="w-full h-[500px] rounded-lg overflow-hidden border border-gray-700"
    />
  );
}
