import './markdown-preview.css';

import { raw } from 'kerfjs';
import { marked } from 'marked';

import {type AttachmentReferenceContext,expandAttachmentReferences,parseAttachmentReference} from '../attachment-references';

export function escapeMarkdownHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function safeUrl(value: string): string {
  const trimmed = value.trim();
  return /^(https?:|mailto:|#|\/)/i.test(trimmed) ? escapeMarkdownHtml(trimmed) : '#';
}

function attachmentUrlInfo(href:string,title?:string|null):{ticket:string;filename:string;id?:string}|undefined {
  const match=/\/tickets\/([^/]+)\/attachments\/by-name\/([^/?#]+)/.exec(href);
  if(match){try{return{ticket:decodeURIComponent(match[1]),filename:decodeURIComponent(match[2])}}catch{return undefined}}
  const direct=/\/tickets\/([^/]+)\/attachments\/([^/?#]+)(?:[/?#]|$)/.exec(href),reference=title&&parseAttachmentReference(title);
  if(!direct||direct[2]==='by-name'||!reference)return undefined;
  try{return{ticket:decodeURIComponent(direct[1]),filename:reference.filename,id:decodeURIComponent(direct[2])}}catch{return undefined}
}

const TICKET_REFERENCE=/\b([A-Z][A-Z0-9]{1,15}-[A-Z0-9]{2,24})\b/g;
const REFERENCE_SUPPRESSING_TAGS=new Set(['a','button','code','pre']);

/** Link plain-text ticket references after Markdown rendering, without touching code or links. */
export function linkTicketReferences(html:string):string {
  let suppressed=0;
  return html.split(/(<[^>]+>)/g).map(part=>{
    if(part.startsWith('<')){
      const match=/^<\/?([a-z0-9]+)/i.exec(part),tag=match?.[1]?.toLocaleLowerCase();
      if(tag&&REFERENCE_SUPPRESSING_TAGS.has(tag))suppressed+=part.startsWith('</')?-1:part.endsWith('/>')?0:1;
      return part;
    }
    if(suppressed>0)return part;
    return part.replace(TICKET_REFERENCE,(reference:string)=>`<a class="markdown-preview__ticket-reference" href="#ticket-${reference}" data-action="open-linked-ticket" data-ticket-slug="${reference}" title="Open ${reference}">${reference}</a>`);
  }).join('');
}

marked.setOptions({ breaks: true, gfm: true });
marked.use({ renderer: {
  html({ text }) { return escapeMarkdownHtml(text); },
  link({ href, title, tokens }) { const info=attachmentUrlInfo(href,title);return `<a href="${safeUrl(href)}" target="_blank" rel="noopener noreferrer"${info?` data-action="open-referenced-attachment" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}"`:''}${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`; },
  image({ href, title, text }) { const info=attachmentUrlInfo(href,title),image=`<img src="${safeUrl(href)}" alt="${escapeMarkdownHtml(text)}"${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>`;return info?`<button type="button" class="markdown-preview__attachment-image" data-action="open-attachment-gallery" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}"${info.id?` data-gallery-attachment-id="${escapeMarkdownHtml(info.id)}"`:''} aria-label="Open ${escapeMarkdownHtml(info.filename)} in image gallery">${image}</button>`:image; },
} });

export function renderMarkdown(source: string,attachmentContext?:AttachmentReferenceContext): string {
  return linkTicketReferences(marked.parse(expandAttachmentReferences(source,attachmentContext), { async: false }));
}

export function MarkdownPreview({ source, emptyLabel = 'Nothing to preview.',attachmentContext }: { source: string; emptyLabel?: string;attachmentContext?:AttachmentReferenceContext }) {
  if (!source.trim()) return <div class="markdown-preview markdown-preview--empty" data-component="markdown-preview">{emptyLabel}</div>;
  return <div class="markdown-preview" data-component="markdown-preview">{raw(renderMarkdown(source,attachmentContext))}</div>;
}
