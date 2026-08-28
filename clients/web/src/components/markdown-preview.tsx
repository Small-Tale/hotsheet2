import './markdown-preview.css';

export function MarkdownPreview({ source, emptyLabel = 'Nothing to preview.' }: { source: string; emptyLabel?: string }) {
  const lines = source.trim().split('\n');
  if (!source.trim()) return <div class="markdown-preview markdown-preview--empty" data-component="markdown-preview">{emptyLabel}</div>;
  return <div class="markdown-preview" data-component="markdown-preview">{lines.map((line, index) => {
    if (line.startsWith('### ')) return <h3 data-line={String(index + 1)}>{line.slice(4)}</h3>;
    if (line.startsWith('## ')) return <h2 data-line={String(index + 1)}>{line.slice(3)}</h2>;
    if (line.startsWith('# ')) return <h1 data-line={String(index + 1)}>{line.slice(2)}</h1>;
    if (line.startsWith('- ')) return <div class="markdown-preview__list-item" data-line={String(index + 1)}><span aria-hidden="true"></span><p>{line.slice(2)}</p></div>;
    if (!line.trim()) return <div class="markdown-preview__break" aria-hidden="true"></div>;
    return <p data-line={String(index + 1)}>{line}</p>;
  })}</div>;
}
