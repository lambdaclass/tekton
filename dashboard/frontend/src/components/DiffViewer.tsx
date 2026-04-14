import { useMemo } from 'react';
import { html, parse } from 'diff2html';
import { ColorSchemeType } from 'diff2html/lib/types';
import 'diff2html/bundles/css/diff2html.min.css';

export default function DiffViewer({ diff }: { diff: string }) {
  const rendered = useMemo(() => {
    const files = parse(diff);
    if (files.length === 0) return '';
    return html(files, {
      outputFormat: 'line-by-line',
      drawFileList: true,
      matching: 'lines',
      colorScheme: ColorSchemeType.AUTO,
    });
  }, [diff]);

  if (!rendered) return null;

  return (
    <>
      <style>{`
        .diff2html-wrapper .d2h-code-linenumber,
        .diff2html-wrapper .d2h-code-side-linenumber {
          position: sticky;
          left: 0;
          z-index: 1;
        }
      `}</style>
      <div
        className="diff2html-wrapper overflow-auto text-sm [&_.d2h-wrapper]:bg-transparent [&_.d2h-file-header]:bg-card [&_.d2h-file-header]:border-border [&_.d2h-code-linenumber]:bg-transparent [&_.d2h-code-line]:bg-transparent [&_.d2h-ins]:bg-emerald-500/10 [&_.d2h-del]:bg-red-500/10 [&_.d2h-file-list-wrapper]:bg-transparent [&_.d2h-file-list-wrapper]:border-border [&_.d2h-tag]:bg-secondary [&_.d2h-tag]:text-foreground [&_.d2h-info]:bg-secondary/50 [&_td]:border-border [&_.d2h-file-wrapper]:border-border [&_.d2h-diff-table]:font-mono"
        dangerouslySetInnerHTML={{ __html: rendered }}
      />
    </>
  );
}
