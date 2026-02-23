import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { connectTaskOutput } from '@/lib/api';
import '@xterm/xterm/css/xterm.css';

interface LogViewerProps {
  /** For task logs: creates its own WS that sends DB history + live updates */
  taskId?: string;
  /** For preview logs: external WS (no DB history) */
  ws?: WebSocket | null;
  onConnectionChange?: (connected: boolean) => void;
}

export default function LogViewer({ taskId, ws, onConnectionChange }: LogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Mode 1: Self-managed WS for task logs (creates WS + terminal together, no race)
  useEffect(() => {
    if (!containerRef.current || !taskId) return;

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

    // Create WS and attach listener in the same synchronous block —
    // WS events are macrotasks and can't fire until this effect finishes,
    // so we never miss the historical logs the server sends on connect.
    const socket = connectTaskOutput(taskId);
    socket.addEventListener('open', () => onConnectionChange?.(true));
    socket.addEventListener('message', (ev) => {
      term.writeln(ev.data);
    });
    socket.addEventListener('close', () => {
      onConnectionChange?.(false);
      term.writeln('\r\n\x1b[90m--- Connection closed ---\x1b[0m');
    });

    const onResize = () => fit.fit();
    window.addEventListener('resize', onResize);

    return () => {
      window.removeEventListener('resize', onResize);
      socket.close();
      term.dispose();
    };
  }, [taskId]);

  // Mode 2: External WS for preview logs (no DB history)
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (taskId || !containerRef.current) return; // skip if using mode 1

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
  }, [taskId]);

  useEffect(() => {
    if (taskId || !ws || !termRef.current) return;

    const term = termRef.current;
    const onMessage = (ev: MessageEvent) => term.writeln(ev.data);
    const onClose = () => term.writeln('\r\n\x1b[90m--- Connection closed ---\x1b[0m');

    ws.addEventListener('message', onMessage);
    ws.addEventListener('close', onClose);

    return () => {
      ws.removeEventListener('message', onMessage);
      ws.removeEventListener('close', onClose);
    };
  }, [taskId, ws]);

  return (
    <div
      ref={containerRef}
      className="w-full h-[500px] overflow-hidden"
    />
  );
}
