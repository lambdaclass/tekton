interface FileDiff {
  filename: string;
  lines: string[];
}

function parseDiff(raw: string): FileDiff[] {
  const files: FileDiff[] = [];
  let current: FileDiff | null = null;

  for (const line of raw.split('\n')) {
    if (line.startsWith('diff --git ')) {
      if (current) files.push(current);
      current = { filename: '', lines: [] };
    } else if (line.startsWith('+++ b/') && current) {
      current.filename = line.slice(6);
    } else if (current && !line.startsWith('--- ') && !line.startsWith('index ') && !line.startsWith('new file') && !line.startsWith('deleted file') && !line.startsWith('diff --git')) {
      current.lines.push(line);
    }
  }
  if (current) files.push(current);
  return files;
}

function lineClass(line: string): string {
  if (line.startsWith('+')) return 'bg-emerald-500/10 text-emerald-300';
  if (line.startsWith('-')) return 'bg-red-500/10 text-red-300';
  if (line.startsWith('@@')) return 'bg-primary/10 text-primary/70';
  return 'text-muted-foreground';
}

export default function DiffViewer({ diff }: { diff: string }) {
  const files = parseDiff(diff);

  return (
    <div className="overflow-auto max-h-[600px] text-xs font-mono rounded-lg border border-border">
      {files.map((file, fi) => (
        <div key={fi}>
          <div className="px-4 py-2 bg-card border-b border-border text-sm font-sans font-medium truncate">
            {file.filename}
          </div>
          <pre className="m-0 p-0 leading-5">
            {file.lines.map((line, li) => (
              <div key={li} className={`px-3 whitespace-pre ${lineClass(line)}`}>
                {line || ' '}
              </div>
            ))}
          </pre>
        </div>
      ))}
    </div>
  );
}
