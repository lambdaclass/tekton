import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { connectTaskOutput, connectPreviewLogs } from '@/lib/api';
import '@xterm/xterm/css/xterm.css';

const ERROR_LINE_RE = /\b(error|exception|fatal|panic|traceback|critical)\b/i;

function writeLine(term: Terminal, line: string) {
  if (ERROR_LINE_RE.test(line)) {
    term.writeln(`\x1b[31m${line}\x1b[0m`);
  } else {
    term.writeln(line);
  }
}

const TERM_OPTIONS = {
  convertEol: true,
  fontSize: 13,
  fontFamily: '"Geist Mono", Menlo, Monaco, "Courier New", monospace',
  theme: {
    background: '#1a1a22',
    foreground: '#c8c8d0',
    cursor: '#c8c8d0',
    selectionBackground: 'rgba(200, 200, 208, 0.15)',
    selectionForeground: '#ffffff',
  },
  scrollback: 10000,
  disableStdin: true,
} as const;

interface LogViewerProps {
  /** For task logs: creates its own WS that sends DB history + live updates */
  taskId?: string;
  /** For preview logs: creates its own WS via connectPreviewLogs */
  previewSlug?: string;
  /** For preview logs: external WS (no DB history) */
  ws?: WebSocket | null;
  onConnectionChange?: (connected: boolean) => void;
}

export default function LogViewer({ taskId, previewSlug, ws, onConnectionChange }: LogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Mode 1: Self-managed WS for task logs with auto-reconnect
  useEffect(() => {
    if (!containerRef.current || !taskId) return;

    const term = new Terminal(TERM_OPTIONS);
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    let disposed = false;
    let currentSocket: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    function connect() {
      if (disposed) return;

      const socket = connectTaskOutput(taskId!);
      currentSocket = socket;

      socket.addEventListener('open', () => {
        // Clear terminal before replaying history to avoid duplicates
        term.clear();
        onConnectionChange?.(true);
      });
      socket.addEventListener('message', (ev) => {
        writeLine(term, ev.data);
      });
      socket.addEventListener('close', () => {
        onConnectionChange?.(false);
        if (!disposed) {
          // Auto-reconnect after 3 seconds
          reconnectTimer = setTimeout(connect, 3000);
        }
      });
    }

    connect();

    const onResize = () => fit.fit();
    window.addEventListener('resize', onResize);

    return () => {
      disposed = true;
      window.removeEventListener('resize', onResize);
      if (reconnectTimer) clearTimeout(reconnectTimer);
      currentSocket?.close();
      term.dispose();
    };
  }, [taskId]);

  // Mode 2: Self-managed WS for preview logs with auto-reconnect
  useEffect(() => {
    if (!containerRef.current || !previewSlug || taskId) return;

    const term = new Terminal(TERM_OPTIONS);
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    let disposed = false;
    let currentSocket: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    function connect() {
      if (disposed) return;

      const socket = connectPreviewLogs(previewSlug!);
      currentSocket = socket;

      socket.addEventListener('open', () => {
        onConnectionChange?.(true);
      });
      socket.addEventListener('message', (ev) => {
        writeLine(term, ev.data);
      });
      socket.addEventListener('close', () => {
        onConnectionChange?.(false);
        if (!disposed) {
          reconnectTimer = setTimeout(connect, 3000);
        }
      });
    }

    connect();

    const onResize = () => fit.fit();
    window.addEventListener('resize', onResize);

    return () => {
      disposed = true;
      window.removeEventListener('resize', onResize);
      if (reconnectTimer) clearTimeout(reconnectTimer);
      currentSocket?.close();
      term.dispose();
    };
  }, [previewSlug, taskId]);

  // Mode 3: External WS for preview logs (legacy)
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (taskId || previewSlug || !containerRef.current) return;

    const term = new Terminal(TERM_OPTIONS);
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
  }, [taskId, previewSlug]);

  useEffect(() => {
    if (taskId || previewSlug || !ws || !termRef.current) return;

    const term = termRef.current;
    const onMessage = (ev: MessageEvent) => writeLine(term, ev.data);
    const onClose = () => term.writeln('\r\n\x1b[90m--- Connection closed ---\x1b[0m');

    ws.addEventListener('message', onMessage);
    ws.addEventListener('close', onClose);

    return () => {
      ws.removeEventListener('message', onMessage);
      ws.removeEventListener('close', onClose);
    };
  }, [taskId, previewSlug, ws]);

  return (
    <div className="rounded-lg border border-border overflow-hidden">
      <div
        ref={containerRef}
        className="w-full h-[500px] overflow-hidden"
      />
    </div>
  );
}
