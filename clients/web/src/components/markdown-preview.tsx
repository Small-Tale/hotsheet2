import './markdown-preview.css';

import { raw } from 'kerfjs';
import { marked } from 'marked';

import {type AttachmentReferenceContext,expandAttachmentReferences} from '../attachment-references';

export function escapeMarkdownHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function safeUrl(value: string): string {
  const trimmed = value.trim();
  return /^(https?:|mailto:|#|\/)/i.test(trimmed) ? escapeMarkdownHtml(trimmed) : '#';
}

function attachmentUrlInfo(href:string):{ticket:string;filename:string}|undefined {
  const match=/\/tickets\/([^/]+)\/attachments\/by-name\/([^/?#]+)/.exec(href);
  if(!match)return undefined;
  try{return{ticket:decodeURIComponent(match[1]),filename:decodeURIComponent(match[2])}}catch{return undefined}
}

marked.setOptions({ breaks: true, gfm: true });
marked.use({ renderer: {
  html({ text }) { return escapeMarkdownHtml(text); },
  link({ href, title, tokens }) { const info=attachmentUrlInfo(href);return `<a href="${safeUrl(href)}" target="_blank" rel="noopener noreferrer"${info?` data-action="open-referenced-attachment" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}"`:''}${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`; },
  image({ href, title, text }) { const info=attachmentUrlInfo(href),image=`<img src="${safeUrl(href)}" alt="${escapeMarkdownHtml(text)}"${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>`;return info?`<button type="button" class="markdown-preview__attachment-image" data-action="open-attachment-gallery" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}" aria-label="Open ${escapeMarkdownHtml(info.filename)} in image gallery">${image}</button>`:image; },
} });

export function renderMarkdown(source: string,attachmentContext?:AttachmentReferenceContext): string {
  return marked.parse(expandAttachmentReferences(source,attachmentContext), { async: false });
}

export function MarkdownPreview({ source, emptyLabel = 'Nothing to preview.',attachmentContext }: { source: string; emptyLabel?: string;attachmentContext?:AttachmentReferenceContext }) {
  if (!source.trim()) return <div class="markdown-preview markdown-preview--empty" data-component="markdown-preview">{emptyLabel}</div>;
  return <div class="markdown-preview" data-component="markdown-preview">{raw(renderMarkdown(source,attachmentContext))}</div>;
}
