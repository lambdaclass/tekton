import { useEffect, useRef } from 'react';
import { html as diff2htmlHtml } from 'diff2html';
import { ColorSchemeType } from 'diff2html/lib/types';
import 'diff2html/bundles/css/diff2html.min.css';

export default function DiffViewer({ diff }: { diff: string }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current || !diff) return;
    ref.current.innerHTML = diff2htmlHtml(diff, {
      drawFileList: false,
      outputFormat: 'line-by-line',
      matching: 'none',
      colorScheme: ColorSchemeType.DARK,
    });
  }, [diff]);

  return <div ref={ref} className="overflow-auto max-h-[500px] text-sm" />;
}
