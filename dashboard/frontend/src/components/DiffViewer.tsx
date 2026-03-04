import { useState } from 'react';

interface FileDiff {
  filename: string;
  lines: string[];
  additions: number;
  deletions: number;
}

function parseDiff(raw: string): FileDiff[] {
  const files: FileDiff[] = [];
  let current: FileDiff | null = null;

  for (const line of raw.split('\n')) {
    if (line.startsWith('diff --git ')) {
      if (current) files.push(current);
      current = { filename: '', lines: [], additions: 0, deletions: 0 };
    } else if (line.startsWith('+++ b/') && current) {
      current.filename = line.slice(6);
    } else if (current && !line.startsWith('--- ') && !line.startsWith('index ') && !line.startsWith('new file') && !line.startsWith('deleted file') && !line.startsWith('diff --git')) {
      current.lines.push(line);
      if (line.startsWith('+')) current.additions++;
      else if (line.startsWith('-')) current.deletions++;
    }
  }
  if (current) files.push(current);
  return files;
}

function lineClass(line: string): string {
  if (line.startsWith('+')) return 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300';
  if (line.startsWith('-')) return 'bg-red-500/10 text-red-700 dark:text-red-300';
  if (line.startsWith('@@')) return 'bg-secondary text-muted-foreground';
  return 'text-muted-foreground';
}

export default function DiffViewer({ diff }: { diff: string }) {
  const files = parseDiff(diff);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());

  const toggle = (index: number) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const expandAll = () => setExpanded(new Set(files.map((_, i) => i)));
  const collapseAll = () => setExpanded(new Set());

  const allExpanded = expanded.size === files.length && files.length > 0;

  return (
    <div className="overflow-auto text-xs font-mono">
      {files.length > 1 && (
        <div className="px-4 py-2 flex justify-end">
          <button
            onClick={allExpanded ? collapseAll : expandAll}
            className="text-xs font-sans text-muted-foreground hover:text-foreground transition-colors"
          >
            {allExpanded ? 'Collapse all' : 'Expand all'}
          </button>
        </div>
      )}
      {files.map((file, fi) => (
        <div key={fi}>
          <button
            onClick={() => toggle(fi)}
            className="w-full px-4 py-2 bg-card border-b border-border text-sm font-sans font-medium flex items-center gap-2 hover:bg-muted/50 transition-colors text-left cursor-pointer"
          >
            <svg
              className={`w-3.5 h-3.5 shrink-0 text-muted-foreground transition-transform ${expanded.has(fi) ? 'rotate-90' : ''}`}
              viewBox="0 0 16 16"
              fill="currentColor"
            >
              <path d="M6.22 3.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06-1.06L9.94 8 6.22 4.28a.75.75 0 0 1 0-1.06Z" />
            </svg>
            <span className="truncate flex-1">{file.filename}</span>
            <span className="shrink-0 font-mono text-xs flex gap-2">
              {file.additions > 0 && <span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span>}
              {file.deletions > 0 && <span className="text-red-600 dark:text-red-400">-{file.deletions}</span>}
            </span>
          </button>
          {expanded.has(fi) && (
            <pre className="m-0 p-0 leading-5">
              {file.lines.map((line, li) => (
                <div key={li} className={`px-3 whitespace-pre ${lineClass(line)}`}>
                  {line || ' '}
                </div>
              ))}
            </pre>
          )}
        </div>
      ))}
    </div>
  );
}
