import './markdown-preview.css';

import { raw } from 'kerfjs';
import { marked } from 'marked';

export function escapeMarkdownHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function safeUrl(value: string): string {
  const trimmed = value.trim();
  return /^(https?:|mailto:|#|\/)/i.test(trimmed) ? escapeMarkdownHtml(trimmed) : '#';
}

marked.setOptions({ breaks: true, gfm: true });
marked.use({ renderer: {
  html({ text }) { return escapeMarkdownHtml(text); },
  link({ href, title, tokens }) { return `<a href="${safeUrl(href)}" target="_blank" rel="noopener noreferrer"${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`; },
  image({ href, title, text }) { return `<img src="${safeUrl(href)}" alt="${escapeMarkdownHtml(text)}"${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>`; },
} });

export function renderMarkdown(source: string): string {
  return marked.parse(source, { async: false });
}

export function MarkdownPreview({ source, emptyLabel = 'Nothing to preview.' }: { source: string; emptyLabel?: string }) {
  if (!source.trim()) return <div class="markdown-preview markdown-preview--empty" data-component="markdown-preview">{emptyLabel}</div>;
  return <div class="markdown-preview" data-component="markdown-preview">{raw(renderMarkdown(source))}</div>;
}
