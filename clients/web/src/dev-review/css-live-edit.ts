import type { ReviewAttachment } from './index';

const SNAPSHOT_MIME = 'text/css';
export interface CssSnapshotSheet { label: string; rules?: readonly string[] }
export interface CssSnapshotInlineStyle { selector: string; cssText: string }

function styleRoots(doc: Document): Array<Document | ShadowRoot> {
  const roots: Array<Document | ShadowRoot> = [doc];
  for (let index = 0; index < roots.length; index += 1) {
    for (const element of roots[index].querySelectorAll('*')) {
      if (element.shadowRoot && !roots.includes(element.shadowRoot)) roots.push(element.shadowRoot);
    }
  }
  return roots;
}

function describeSheet(sheet: CSSStyleSheet, index: number): string {
  if (sheet.href) return sheet.href;
  const owner = sheet.ownerNode;
  if (owner instanceof Element) {
    const id = owner.id ? `#${owner.id}` : '';
    return `inline <${owner.tagName.toLowerCase()}${id}>`;
  }
  return `constructed stylesheet ${index + 1}`;
}

function selectorSegment(element: Element): string {
  const tag = element.tagName.toLowerCase();
  if (element.id) return `${tag}#${element.id.replaceAll(/[^a-zA-Z0-9_-]/g, '\\$&')}`;
  const parent = element.parentElement;
  if (!parent) return tag;
  const siblings = [...parent.children].filter(sibling => sibling.tagName === element.tagName);
  return siblings.length > 1 ? `${tag}:nth-of-type(${siblings.indexOf(element) + 1})` : tag;
}

function elementPath(element: Element): string {
  const segments: string[] = [];
  let current: Element | null = element;
  while (current) {
    segments.unshift(selectorSegment(current));
    if (current.id) break;
    const root = current.getRootNode();
    current = current.parentElement ?? (root instanceof ShadowRoot ? root.host : null);
    if (current && root instanceof ShadowRoot) segments.unshift('::shadow');
  }
  return segments.join(' > ');
}

/**
 * Serialize the live CSSOM rather than stylesheet source text, so edits made in browser
 * developer tools are represented even when they never update a `<style>` node's text.
 */
export function captureCssSnapshot(doc: Document): string {
  const roots = styleRoots(doc);
  const sheets: CSSStyleSheet[] = [];
  const seen = new Set<CSSStyleSheet>();
  const add = (sheet: CSSStyleSheet | null | undefined) => {
    if (!sheet || seen.has(sheet)) return;
    seen.add(sheet);
    sheets.push(sheet);
    try {
      for (const rule of sheet.cssRules) if ('styleSheet' in rule) add((rule as CSSImportRule).styleSheet);
    } catch { /* Cross-origin rules remain represented by their unavailable sheet label. */ }
  };
  for (const sheet of doc.styleSheets) add(sheet);
  for (const root of roots) {
    for (const node of root.querySelectorAll<HTMLStyleElement | HTMLLinkElement>('style,link[rel="stylesheet"]')) add(node.sheet);
    for (const sheet of root.adoptedStyleSheets) add(sheet);
  }

  const stylesheetSnapshots = sheets.map((sheet, index): CssSnapshotSheet => {
    try {
      return { label: describeSheet(sheet, index), rules: [...sheet.cssRules].map(rule => rule.cssText) };
    } catch {
      return { label: `${describeSheet(sheet, index)} (rules unavailable to the browser)` };
    }
  });
  const inline = roots.flatMap(root => [...root.querySelectorAll<HTMLElement>('[style]')])
    .filter(element => element.style.length > 0)
    .map(element => ({ selector: elementPath(element), cssText: element.style.cssText }));
  return formatCssSnapshot(stylesheetSnapshots, inline);
}

export function formatCssSnapshot(sheets: readonly CssSnapshotSheet[], inline: readonly CssSnapshotInlineStyle[]): string {
  const blocks = sheets.map((sheet, index) => `/* stylesheet ${index + 1}: ${sheet.label} */${sheet.rules ? `\n${sheet.rules.join('\n')}` : ''}`);
  if (inline.length > 0) blocks.push(`/* inline style declarations */\n${inline.map(style => `/* inline style: ${style.selector} */\n${style.selector} { ${style.cssText} }`).join('\n')}`);
  return `${blocks.join('\n\n')}\n`;
}

function base64(view: Window, value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += 0x8000) binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  return view.btoa(binary);
}

export function cssSnapshotAttachment(view: Window, filename: string, snapshot: string): ReviewAttachment {
  return {
    id: filename,
    filename,
    dataUrl: `data:${SNAPSHOT_MIME};base64,${base64(view, snapshot)}`,
    mimeType: SNAPSHOT_MIME,
    size: new TextEncoder().encode(snapshot).byteLength,
  };
}

export const CSS_LIVE_EDIT_TICKET_NOTES = `Apply the captured CSS Live Edit changes.

Compare the attached css-live-edit-before.css and css-live-edit-after.css snapshots, identify the intentional CSS differences, and reproduce those design changes in the repository's source styles and components. Treat the snapshots as evidence of the desired result, not as generated files to copy wholesale.`;
