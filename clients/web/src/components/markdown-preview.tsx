import './markdown-preview.css';

import { raw } from 'kerfjs';
import { marked } from 'marked';

import {
  type AttachmentReferenceContext,
  expandAttachmentReferences,
  parseAttachmentReference,
} from '../attachment-references';
import { parseTicketLinkReference, type TicketLinkReference, ticketReferencePattern } from '../ticket-link-resolution';

export function escapeMarkdownHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function safeUrl(value: string): string {
  const trimmed = value.trim();
  return /^(https?:|mailto:|#|\/)/i.test(trimmed) ? escapeMarkdownHtml(trimmed) : '#';
}

function attachmentUrlInfo(
  href: string,
  title?: string | null,
): { ticket: string; filename: string; id?: string } | undefined {
  const match = /\/tickets\/([^/]+)\/attachments\/by-name\/([^/?#]+)/.exec(href);
  if (match) {
    try {
      return { ticket: decodeURIComponent(match[1]), filename: decodeURIComponent(match[2]) };
    } catch {
      return undefined;
    }
  }
  const direct = /\/tickets\/([^/]+)\/attachments\/([^/?#]+)(?:[/?#]|$)/.exec(href),
    reference = title && parseAttachmentReference(title);
  if (!direct || direct[2] === 'by-name' || !reference) return undefined;
  try {
    return { ticket: decodeURIComponent(direct[1]), filename: reference.filename, id: decodeURIComponent(direct[2]) };
  } catch {
    return undefined;
  }
}

const REFERENCE_SUPPRESSING_TAGS = new Set(['a', 'button', 'code', 'pre']);

function ticketReferenceAnchor(reference: TicketLinkReference, inner: string): string {
  return `<a class="markdown-preview__ticket-reference" href="#ticket-${reference.slug}" data-action="open-linked-ticket" data-ticket-slug="${reference.slug}"${reference.projectId ? ` data-ticket-project-id="${reference.projectId}"` : ''} title="Open ${reference.raw}">${inner}</a>`;
}

/**
 * Link plain-text ticket references after Markdown rendering, without touching links, buttons,
 * or code blocks. Inline code whose whole content is one ticket reference (the `HS2-XXXX` form
 * notes use for slugs) becomes a link around the code chip (HS2-5T33YV); other code stays inert.
 */
export function linkTicketReferences(html: string): string {
  let suppressed = 0;
  const parts = html.split(/(<[^>]+>)/g),
    output: string[] = [];
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    if (part.startsWith('<')) {
      const codeText = parts[index + 1],
        reference =
          suppressed === 0 && part === '<code>' && parts[index + 2] === '</code>' && codeText
            ? parseTicketLinkReference(codeText)
            : undefined;
      if (reference && reference.raw === codeText) {
        output.push(ticketReferenceAnchor(reference, `<code>${codeText}</code>`));
        index += 2;
        continue;
      }
      const match = /^<\/?([a-z0-9]+)/i.exec(part),
        tag = match?.[1]?.toLocaleLowerCase();
      if (tag && REFERENCE_SUPPRESSING_TAGS.has(tag))
        suppressed += part.startsWith('</') ? -1 : part.endsWith('/>') ? 0 : 1;
      output.push(part);
      continue;
    }
    output.push(
      suppressed > 0
        ? part
        : part.replace(ticketReferencePattern(), (raw: string) =>
            ticketReferenceAnchor(parseTicketLinkReference(raw)!, raw),
          ),
    );
  }
  return output.join('');
}

marked.setOptions({ breaks: true, gfm: true });
marked.use({
  renderer: {
    html({ text }) {
      return escapeMarkdownHtml(text);
    },
    link({ href, title, tokens }) {
      const info = attachmentUrlInfo(href, title);
      return `<a href="${safeUrl(href)}" target="_blank" rel="noopener noreferrer"${info ? ` data-action="open-referenced-attachment" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}"` : ''}${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`;
    },
    image({ href, title, text }) {
      const info = attachmentUrlInfo(href, title),
        image = `<img src="${safeUrl(href)}" alt="${escapeMarkdownHtml(text)}"${title ? ` title="${escapeMarkdownHtml(title)}"` : ''}>`;
      return info
        ? `<button type="button" class="markdown-preview__attachment-image" data-action="open-attachment-gallery" data-attachment-url="${safeUrl(href)}" data-attachment-ticket="${escapeMarkdownHtml(info.ticket)}" data-attachment-name="${escapeMarkdownHtml(info.filename)}"${info.id ? ` data-gallery-attachment-id="${escapeMarkdownHtml(info.id)}"` : ''} aria-label="Open ${escapeMarkdownHtml(info.filename)} in image gallery">${image}</button>`
        : image;
    },
  },
});

export function renderMarkdown(source: string, attachmentContext?: AttachmentReferenceContext): string {
  return linkTicketReferences(marked.parse(expandAttachmentReferences(source, attachmentContext), { async: false }));
}

/** Text color: the reading color, the container's color, or the container's color for links too. */
export type MarkdownPreviewTone = 'default' | 'inherit' | 'inverse';
/** Type scale: the reading size, a small secondary size, or the container's font. */
export type MarkdownPreviewSize = 'default' | 'small' | 'inherit';
/** Spacing between block elements: reading rhythm, a tight rhythm, or none. */
export type MarkdownPreviewDensity = 'default' | 'compact' | 'flush';
/** Attachment image presentation: full width or a fixed, cropped thumbnail. */
export type MarkdownPreviewMedia = 'full' | 'thumbnail';

export interface MarkdownPreviewProps {
  source: string;
  emptyLabel?: string;
  attachmentContext?: AttachmentReferenceContext;
  /** `inverse` keeps text and links in the container's color (for example on a brand fill). */
  tone?: MarkdownPreviewTone;
  size?: MarkdownPreviewSize;
  density?: MarkdownPreviewDensity;
  media?: MarkdownPreviewMedia;
}

/**
 * Sanitized Markdown rendering. Consumers choose presentation through `tone`, `size`, `density`,
 * and `media` rather than restyling `.markdown-preview` from their own stylesheets.
 */
export function MarkdownPreview({
  source,
  emptyLabel = 'Nothing to preview.',
  attachmentContext,
  tone = 'default',
  size = 'default',
  density = 'default',
  media = 'full',
}: MarkdownPreviewProps) {
  const presentation = {
    'data-component': 'markdown-preview',
    'data-tone': tone === 'default' ? undefined : tone,
    'data-size': size === 'default' ? undefined : size,
    'data-density': density === 'default' ? undefined : density,
    'data-media': media === 'full' ? undefined : media,
  };
  if (!source.trim())
    return (
      <div class="markdown-preview markdown-preview--empty" {...presentation}>
        {emptyLabel}
      </div>
    );
  // renderMarkdown escapes raw HTML and constrains URL schemes before this deliberately raw Kerf
  // rendering boundary, so the dynamic value is already sanitized (documented at renderMarkdown).
  const rendered = renderMarkdown(source, attachmentContext);
  return (
    <div class="markdown-preview" {...presentation}>
      {/* eslint-disable-next-line kerfjs/no-raw-with-dynamic-arg -- sanitized upstream by renderMarkdown */}
      {raw(rendered)}
    </div>
  );
}
