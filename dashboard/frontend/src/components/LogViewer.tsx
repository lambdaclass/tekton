import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { connectTaskOutput, connectPreviewLogs, connectAutoresearchOutput } from '@/lib/api';
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
  /** For autoresearch run logs */
  autoresearchRunId?: string;
  /** For autoresearch: current run status, used to disable WS auto-reconnect
   *  on terminal states (otherwise the backend's post-cleanup channel removal
   *  causes an endless reconnect → re-replay → terminal-clear loop). */
  autoresearchRunStatus?: string;
  /** For preview logs: external WS (no DB history) */
  ws?: WebSocket | null;
  onConnectionChange?: (connected: boolean) => void;
}

export default function LogViewer({ taskId, previewSlug, autoresearchRunId, autoresearchRunStatus, ws, onConnectionChange }: LogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  // Read inside the WS close handler so we don't tear down + recreate the
  // socket every time the parent re-renders with a fresh status.
  const autoresearchRunStatusRef = useRef(autoresearchRunStatus);
  autoresearchRunStatusRef.current = autoresearchRunStatus;

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
        // Clear terminal before replaying history to avoid duplicates on reconnect
        term.clear();
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

  // Mode: Self-managed WS for autoresearch run logs with auto-reconnect
  useEffect(() => {
    if (!containerRef.current || !autoresearchRunId || taskId || previewSlug) return;

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

      const socket = connectAutoresearchOutput(autoresearchRunId!);
      currentSocket = socket;

      socket.addEventListener('open', () => {
        term.clear();
        onConnectionChange?.(true);
      });
      socket.addEventListener('message', (ev) => {
        writeLine(term, ev.data);
      });
      socket.addEventListener('close', () => {
        onConnectionChange?.(false);
        if (disposed) return;
        // If the run has finished, the backend's channel will be gone and
        // every reconnect just replays the DB history and closes again,
        // wiping any text selection. Stop reconnecting on terminal states.
        const status = autoresearchRunStatusRef.current;
        const isTerminal = status === 'completed' || status === 'failed' || status === 'stopped';
        if (isTerminal) return;
        reconnectTimer = setTimeout(connect, 3000);
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
  }, [autoresearchRunId, taskId, previewSlug]);

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
    <div className="overflow-hidden h-full">
      <div
        ref={containerRef}
        className="w-full h-full overflow-hidden"
      />
    </div>
  );
}
