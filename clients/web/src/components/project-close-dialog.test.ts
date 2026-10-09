import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  ProjectCloseDialog,
  type ProjectCloseResource,
  projectCloseResourceKey,
  projectCloseRunningSummary,
  selectedProjectCloseResource,
} from './project-close-dialog';

const resources: ProjectCloseResource[] = [
  {
    kind: 'terminal',
    id: 'term-one',
    name: 'Tests',
    busy: true,
    cwd: '/work/demo',
    progress: 68,
    preview: 'Test Files 42 passed',
  },
  { kind: 'terminal', id: 'term-two', name: 'Server', cwd: '/work/demo/server' },
  {
    kind: 'ai-chat',
    id: 'chat-one',
    name: 'Codex chat',
    tool: 'Codex',
    model: 'gpt-6-astra',
    effort: 'high',
    sessionId: 'thread-1',
    messages: [
      { id: 'question', role: 'user', content: 'Review the close flow.' },
      {
        id: 'answer',
        role: 'assistant',
        content: 'The shared transcript stays exact.',
        status: 'completed',
        usage: { tokensIn: 120, tokensOut: 42 },
      },
    ],
    activity: [{ id: 'activity', tool: 'Codex', kind: 'edit', summary: 'Reviewed the dialog', importance: 'normal' }],
    totalUsage: { tokensIn: 120, tokensOut: 42 },
  },
];

describe('ProjectCloseDialog', () => {
  it('stays absent until a project-close decision is requested', () => {
    expect(String(ProjectCloseDialog({}))).toBe('');
  });

  it('uses shared menu primitives and the exact shared read-only conversation for a selected chat', () => {
    const markup = String(
      ProjectCloseDialog({
        state: { projectId: 'demo', projectName: 'Demo', resources, selectedKey: 'ai-chat:chat-one' },
      }),
    );
    expect(markup).toContain('data-component="project-close-dialog"');
    expect(markup).toContain('label="Close Demo?"');
    expect(markup).toContain('2 running terminals and 1 AI chat will stay active unless you close them first.');
    expect(markup).toContain('aria-label="Running terminals and AI chats"');
    expect(markup).toContain('data-component="list-header"');
    expect(markup.match(/data-component="list-item"/g)).toHaveLength(3);
    expect(markup).toContain('data-item-id="ai-chat:chat-one"');
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('aria-label="Codex chat chat preview"');
    expect(markup).toContain('data-key="project-close-preview:ai-chat:chat-one"');
    expect(markup).toContain('data-component="ai-conversation"');
    expect(markup).toContain('data-read-only="true"');
    expect(markup).toContain('Read-only preview');
    expect(markup).toContain('Review the close flow.');
    expect(markup).toContain('The shared transcript stays exact.');
    expect(markup).toContain('Reviewed the dialog');
    expect(markup).toContain('162 tokens');
    expect(markup).not.toContain('send-conversation-turn');
    expect(markup).not.toContain('data-action="save-conversation"');
    expect(markup).not.toContain('thread-1');
    expect(markup).toContain('data-lucide="message-square"');
  });

  it('falls back to the first item and embeds the live read-only terminal renderer', () => {
    const markup = String(
      ProjectCloseDialog({ state: { projectId: 'demo', projectName: 'Demo', resources, selectedKey: 'missing' } }),
    );
    expect(markup).toContain('data-item-id="terminal:term-one"');
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('aria-label="Tests terminal preview"');
    expect(markup).toContain('data-key="project-close-preview:terminal:term-one"');
    expect(markup).toContain('data-component="terminal-viewport"');
    expect(markup).toContain('data-project-id="demo"');
    expect(markup).toContain('data-terminal-id="term-one"');
    expect(markup).toContain('data-display-mode="scaled-preview"');
    // Composed through the shared TerminalPreview, which owns the frame and connecting fallback (HS2-148B5C).
    expect(markup).toContain('data-component="terminal-preview"');
    expect(markup).toContain('data-key="project-close:term-one"');
    expect(markup).toContain('Connecting to the live terminal…');
    expect(markup).not.toContain('terminal-tile__viewport-frame');
    expect(markup).not.toContain('/work/demo');
    expect(markup).not.toContain('68%');
    expect(markup).not.toContain('Test Files 42 passed');
    expect(markup).toContain('data-lucide="square-terminal"');
  });

  it('names the keep-running and stop-all consequences explicitly with busy states', () => {
    const ready = String(ProjectCloseDialog({ state: { projectId: 'demo', projectName: 'Demo', resources } }));
    for (const action of ['cancel-project-close', 'close-all-project-resources', 'confirm-close-project'])
      expect(ready).toContain(`data-action="${action}"`);
    expect(ready).toContain('Stop &amp; Close');
    expect(ready).toContain('Keep Running');
    expect(ready).toContain('Terminals and AI chat tabs return when reopened');
    expect(ready).toContain(
      'Received chat history and the latest durable provider session return after an app or server restart',
    );
    const busy = String(
      ProjectCloseDialog({
        state: {
          projectId: 'demo',
          projectName: 'Demo',
          resources,
          operation: 'closing-all',
          error: 'Could not close Server.',
        },
      }),
    );
    expect(busy).toContain('aria-busy="true"');
    expect(busy).toContain('Stopping…');
    expect(busy).toContain('role="alert"');
    expect(busy).toContain('Could not close Server.');
    expect(busy.match(/disabled/g)?.length).toBeGreaterThanOrEqual(6);
  });

  it('asks for a compact explicit confirmation when nothing is running', () => {
    const markup = String(ProjectCloseDialog({ state: { projectId: 'demo', projectName: 'Demo', resources: [] } }));
    expect(markup).toContain('data-has-resources="false"');
    expect(markup).toContain('Close this project tab? You can reopen it later.');
    expect(markup).toContain('Close Project');
    expect(markup).not.toContain('No terminals or AI chats');
    expect(markup).not.toContain('circle-alert');
    expect(markup).not.toContain('close-all-project-resources');
    expect(markup).not.toContain('Running items');
  });

  it('preserves identity, fallback selection, summary, and owned asymmetric preview geometry', () => {
    expect(projectCloseResourceKey(resources[2])).toBe('ai-chat:chat-one');
    expect(selectedProjectCloseResource(resources, 'terminal:term-two')).toBe(resources[1]);
    expect(selectedProjectCloseResource(resources, 'unknown')).toBe(resources[0]);
    expect(selectedProjectCloseResource([], 'unknown')).toBeUndefined();
    expect(projectCloseRunningSummary([resources[2]])).toBe('1 AI chat will stay active unless you close them first.');
    const css = readFileSync(resolve(import.meta.dirname, 'project-close-dialog.css'), 'utf8');
    expect(css).toMatchSource(/grid-template-columns:minmax\(remify\(224px\),remify\(288px\)\) minmax\(0,1fr\)/);
    expect(css).toMatchSource(/@media \(max-width:remify\(672px\)\)[\s\S]*grid-template-columns:1fr/);
    expect(css).not.toContain('.terminal-viewport');
    expect(css).not.toContain('.terminal-tile');
    expect(css).not.toContain('.ai-conversation');
    expect(css).not.toContain('.kui-');
    expect(css).not.toContain('__intro');
    expect(css).not.toContain('__actions');
    expect(css).not.toContain('__consequences');
  });

  it('composes warnings, scrollable resources, metadata, consequences, and footer through public primitives', () => {
    const markup = String(ProjectCloseDialog({ state: { projectId: 'demo', projectName: 'Demo', resources } }));
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('data-tone="warning"');
    expect(markup).toContain('id="project-close-dialog-summary"');
    expect(markup).toContain('data-scrollable="true"');
    expect(markup).toContain('data-control-insets="trbl"');
    expect(markup).toContain('data-project-close-consequences');
    expect(markup).toContain('slot="footer"');
    expect(markup).toContain('data-component="row"');
  });

  it('gives the phone preview frame its published grid aspect and delegates shell geometry to DialogSurface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'project-close-dialog.css'), 'utf8'),
      phone = css.slice(css.indexOf('@media (max-width: remify(672px))')),
      markup = String(ProjectCloseDialog({ state: { projectId: 'demo', projectName: 'Demo', resources } }));
    expect(phone).toMatchSource(/__layout \{[^}]*grid-template-rows:auto minmax\(remify\(192px\), 1fr\)/);
    expect(phone).toMatchSource(/__resources \{[^}]*max-height:remify\(160px\)/);
    expect(phone).toMatchSource(
      /__terminal \{[^}]*box-sizing:content-box[^}]*max-height:remify\(224px\)[^}]*aspect-ratio:var\(--terminal-preview-grid-aspect, 5 \/ 3\)/,
    );
    expect(css).not.toContain('::part(');
    expect(css).not.toContain('--width:');
    expect(markup).toContain('data-component="dialog-surface"');
    expect(markup).toContain('data-body-inset="none"');
    expect(markup).toContain('data-preferred-width');
    expect(markup).toContain('data-viewport-gutter');
  });
});
