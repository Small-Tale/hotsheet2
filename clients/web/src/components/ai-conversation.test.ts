import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import type { PermissionItem } from '../permission-notifications';
import { AIConversation, isConversationSurfaceLifecycleEvent } from './ai-conversation';
import { PermissionRequestPopup } from './permission-request-card';

const permission: PermissionItem = {
  id: 1,
  connection: 'connection-1',
  tool: 'Bash',
  action: 'npm test',
  key: 'project:1',
  projectId: 'project',
  projectName: 'Project',
  agent: 'Codex',
  role: 'main worker',
  receivedAt: 1,
  ignored: false,
  always_allow_supported: true,
};
const css = readFileSync(new URL('./ai-conversation.css', import.meta.url), 'utf8');
const previewCss = readFileSync(new URL('./markdown-preview.css', import.meta.url), 'utf8');

describe('AIConversation', () => {
  it.each(['dialog', 'embedded'] as const)(
    'keeps native toolbar actions and eligibility in the %s presentation',
    (presentation) => {
      const render = (busy: boolean, interruptible: boolean, populated: boolean, preview = false) =>
        String(
          AIConversation({
            open: true,
            presentation,
            tool: 'Codex',
            draft: '',
            busy,
            interruptible,
            messages: populated ? [{ id: 'answer', role: 'assistant', content: 'Done.', status: 'completed' }] : [],
            readOnly: preview,
            readOnlyContext: 'preview',
          }),
        );
      const save = (markup: string) => markup.match(/<button[^>]*data-action="save-conversation"[^>]*>/)?.[0];
      const ready = render(false, false, true);
      expect(save(ready)).toContain('type="button"');
      expect(save(ready)).not.toContain('disabled');
      expect(ready).toContain('data-component="toolbar-control-group"');
      expect(save(render(false, false, false))).toContain('disabled');
      const busy = render(true, true, true);
      expect(save(busy)).toContain('disabled');
      expect(busy).toMatch(/<button[^>]*type="button"[^>]*data-action="stop-conversation"/);
      expect(render(true, false, true)).not.toContain('data-action="stop-conversation"');
      expect(ready).not.toContain('data-action="stop-conversation"');
      expect(render(false, false, true, true)).not.toContain('data-action="save-conversation"');
    },
  );
  it.each(['dialog', 'embedded'] as const)(
    'shows a labeled spinner, not an empty bubble, while a %s turn is thinking (HS2-77KJKR)',
    (presentation) => {
      const render = (content: string, status: 'streaming' | 'completed') =>
        String(
          AIConversation({
            open: true,
            presentation,
            tool: 'Claude',
            draft: '',
            busy: status === 'streaming',
            interruptible: false,
            progress: 'Reviewing the project and planning the next steps…',
            messages: [
              { id: 'question', role: 'user', content: 'What day is it?' },
              { id: 'answer', role: 'assistant', content, status },
            ],
          }),
        );
      const pending = render('', 'streaming');
      expect(pending).toMatch(
        /data-message-id="answer"[\s\S]*class="ai-conversation__awaiting" role="status"[\s\S]*kui-loading-spinner/,
      );
      expect(pending).toContain('aria-label="Awaiting response"');
      expect(pending).not.toContain('ai-conversation__empty-message');
      // Once text streams in, the spinner gives way to the reply.
      const streaming = render('Today is Thursday.', 'streaming');
      expect(streaming).not.toContain('ai-conversation__awaiting');
      expect(streaming).toContain('Today is Thursday.');
      // A finished empty turn never spins.
      const finished = render('', 'completed');
      expect(finished).not.toContain('ai-conversation__awaiting');
      expect(finished).toContain('ai-conversation__empty-message');
    },
  );
  it('renders ordered Markdown turns, progress, and an inline permission request', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        sessionId: 'session-1',
        messages: [
          { id: 'one', role: 'user', content: 'Please **check** this.' },
          {
            id: 'two',
            role: 'assistant',
            content: 'Checking `main.tsx`.',
            status: 'streaming',
            usage: { tokensIn: 12_000, tokensOut: 840, costUsd: 0.0423, model: 'codex-5.6' },
          },
        ],
        draft: 'Follow up',
        busy: true,
        progress: 'Waiting for permission…',
        interruptible: true,
        permissions: [permission],
        activity: [{ id: 'activity-1', tool: 'Codex', kind: 'edit', summary: 'Edited main.tsx', importance: 'normal' }],
        totalUsage: { tokensIn: 12_000, tokensOut: 840, costUsd: 0.0423 },
        feedbackAvailable: true,
      }),
    );
    expect(markup.indexOf('Please <strong>check</strong> this.')).toBeLessThan(
      markup.indexOf('Checking <code>main.tsx</code>.'),
    );
    expect(markup).toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-component="permission-request-card"');
    expect(markup).toContain('Waiting for permission');
    expect(markup).toContain('12.8K tokens');
    expect(markup).toContain('≈$0.04');
    expect(markup).toContain('Edited main.tsx');
    expect(markup).toContain('AI-generated');
    expect(markup).toContain('may contain errors');
    expect(markup).toContain('aria-label="AI-generated response by Codex"');
    expect(markup).not.toContain('AI-generated by Codex</');
    expect(markup).toContain('data-ai-feedback-target="activity:activity-1"');
    expect(markup).toContain('without-header');
    expect(markup).toContain('light-dismiss');
    expect(markup).not.toContain('data-action="close-conversation"');
    // The heading's supporting line is Kerf ToolbarText's description (HS2-XHC39W).
    expect(markup).toMatch(/data-has-description="true"[\s\S]*kui-toolbar-text__description/);
    // The DialogHeader groups the conversation actions under a localized label (HS2-M4X0WS).
    expect(markup).toMatch(/kui-toolbar__trailing[\s\S]*aria-label="Codex Conversation Actions"/);
    // The familiar Enter-to-send pattern needs no visible hint; the field still announces the
    // shortcut to assistive technology (HS2-3VQSZD).
    expect(markup).not.toContain('Enter to send');
    expect(markup).not.toContain('aria-describedby="ai-conversation-draft-dialog-hint"');
    expect(markup).toMatch(/aria-keyshortcuts="[^"]*Enter/);
    expect(markup).toContain('class="ai-conversation__send"');
    expect(markup).toContain('data-lucide="arrow-up"');
    expect(markup).toContain('Codex is working');
    expect(markup).not.toContain('Session session-1');
  });

  it('renders permission-paused activity before its later assistant result', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Claude',
        messages: [
          { id: 'question', role: 'user', content: 'What time is it?', sequence: 0 },
          { id: 'answer', role: 'assistant', content: 'It is 3:23 PM.', status: 'completed', sequence: 2 },
        ],
        activity: [
          {
            id: 'date',
            tool: 'Claude',
            kind: 'command',
            summary: 'claude ran `date`',
            importance: 'normal',
            sequence: 1,
          },
        ],
        draft: '',
        busy: false,
        interruptible: false,
      }),
    );
    expect(markup.indexOf('What time is it?')).toBeLessThan(markup.indexOf('claude ran'));
    expect(markup.indexOf('claude ran')).toBeLessThan(markup.indexOf('It is 3:23 PM.'));
  });

  it('labels an activity group once, keeps per-entry feedback, and renders commands as contained code', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        messages: [{ id: 'question', role: 'user', content: 'Run the suite.', sequence: 0 }],
        activity: [
          {
            id: 'command',
            tool: 'Codex',
            kind: 'command',
            summary: "Codex ran `zsh -ic 'npm run test -- --project chromium --grep a-deliberately-long-filter'`",
            importance: 'normal',
            sequence: 1,
          },
        ],
        draft: '',
        busy: false,
        interruptible: false,
        feedbackAvailable: true,
      }),
    );
    expect(markup.match(/data-component="ai-content-label"/g)).toHaveLength(1);
    expect(markup).toContain('aria-label="AI-generated; may contain errors"');
    expect(markup).not.toContain('aria-label="AI-generated by Codex; may contain errors"');
    expect(markup).toContain('data-ai-feedback-target="activity:command"');
    expect(markup).toContain(
      '<code>zsh -ic &#39;npm run test -- --project chromium --grep a-deliberately-long-filter&#39;</code>',
    );
    // A Kerf ContentItem frames the activity card; the app grid only lays out its own body (HS2-ADM62E).
    expect(markup).toMatch(/data-component="content-item" data-appearance="neutral"[^>]*role="region"/);
    expect(css).toMatch(/\.ai-conversation__activity-body \{[^}]*display: grid;[^}]*min-width: 0/);
    expect(css).not.toMatch(/\.ai-conversation__activity \{/);
    expect(css).toMatchSource(
      /\.ai-conversation__activity-item \{[^}]*min-width: 0[^}]*max-width: 100%[^}]*grid-template-columns:minmax\(0,1fr\) auto/,
    );
    // Activity summaries choose the flush MarkdownPreview density; long commands wrap through the
    // preview's own inherited overflow-wrap rather than a conversation-owned override (HS2-7RY5GK).
    expect(markup).toMatch(/class="markdown-preview" data-component="markdown-preview" data-density="flush"/);
    expect(markup).toContain('<span class="ai-conversation__activity-feedback">');
    expect(previewCss).toMatchSource(/\.markdown-preview \{[^}]*overflow-wrap:anywhere/);
    expect(css).not.toContain('.markdown-preview');
    expect(css).not.toContain('.ai-content-label');
  });

  it('hides stop when interruption is unavailable and exposes terminal failures', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        messages: [{ id: 'one', role: 'assistant', content: 'The turn ended.', status: 'failed' }],
        draft: '',
        busy: true,
        progress: 'Working…',
        interruptible: false,
        error: 'The tool turn failed.',
      }),
    );
    expect(markup).not.toContain('data-action="stop-conversation"');
    expect(markup).toContain('data-status="failed"');
    expect(markup).toContain('Conversation Unavailable');
    expect(markup).toContain('circle-alert');
    expect(markup).toContain('The tool turn failed.');
  });

  it('lets embedded conversations shrink within a narrow drawer', () => {
    expect(css).toMatch(
      /\.ai-conversation--embedded \{[^}]*width: 100%[^}]*min-width: 0[^}]*max-width: 100%[^}]*height: 100%[^}]*overflow: clip[^}]*box-sizing: border-box/,
    );
  });

  it('only offers persisted ticket feedback when the composition has a ticket note target', () => {
    const message = { id: 'answer', role: 'assistant' as const, content: 'Done.', status: 'completed' as const };
    expect(
      String(
        AIConversation({
          open: true,
          tool: 'Codex',
          messages: [message],
          draft: '',
          busy: false,
          interruptible: false,
        }),
      ),
    ).not.toContain('rate-ai-content');
    expect(
      String(
        AIConversation({
          open: true,
          tool: 'Codex',
          messages: [message],
          draft: '',
          busy: false,
          interruptible: false,
          feedbackAvailable: true,
        }),
      ),
    ).toContain('data-ai-feedback-target="conversation:answer"');
  });

  it('renders structured attachment and media references inside the shared message bubble', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        messages: [
          {
            id: 'answer',
            role: 'assistant',
            content: 'Files attached.',
            files: [
              {
                id: 'doc',
                filename: 'report.pdf',
                mime_type: 'application/pdf',
                kind: 'attachment',
                url: '/files/report',
              },
              { id: 'image', filename: 'proof.png', mime_type: 'image/png', kind: 'media', url: '/files/proof' },
            ],
          },
        ],
        draft: '',
        busy: false,
        interruptible: false,
      }),
    );
    expect(markup).toContain('aria-label="Referenced files"');
    expect(markup).toContain('data-lucide="paperclip"');
    expect(markup).toContain('data-lucide="image"');
    expect(markup).toContain('href="/files/report" download="report.pdf"');
    expect(markup).toMatch(/<a [^>]*data-component="chip"|<a [^>]*class="kui-chip/);
  });

  it('reuses the complete conversation surface as embedded drawer content', () => {
    const markup = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [],
        draft: 'Ask about the project',
        busy: false,
        interruptible: false,
      }),
    );
    expect(markup).toContain('data-presentation="embedded"');
    expect(markup).toContain('ai-conversation--embedded');
    expect(markup).toContain('Conversation Transcript');
    expect(markup).toContain('send-conversation-turn');
    expect(markup).toContain('data-component="composer"');
    expect(markup).toContain('data-submit-on-enter="true"');
    expect(markup).toContain('aria-keyshortcuts="Enter"');
    expect(markup).toContain('class="ai-conversation__send"');
    expect(markup).toContain('data-lucide="arrow-up"');
    expect(markup).not.toContain('wa-dialog');
  });
  it('floats the Kerf Composer over a padded scrolling transcript without app composer chrome (HS2-94T958)', () => {
    expect(css).toMatch(
      /\.ai-conversation--embedded \{[^}]*display: flex[^}]*height: 100%[^}]*min-height: 0[^}]*overflow: clip[^}]*flex-direction: column/,
    );
    // The scroller is a Kerf List styled only through props; the app pads its own content column
    // (HS2-HVZVEA).
    expect(css).not.toMatch(/\.ai-conversation__transcript \{/);
    expect(css).toMatch(
      /\.ai-conversation__transcript-content \{[^}]*padding: remify\(16px\)[^}]*box-sizing: border-box/,
    );
    expect(css).toMatch(
      /\.ai-conversation--embedded\[data-read-only='false'\] \.ai-conversation__transcript-content \{[^}]*padding-bottom: remify\(176px\)/,
    );
    // Kerf owns the dialog body layout, floating footer, composer frame, and file chips.
    expect(css).not.toContain('::part(');
    expect(css).not.toMatch(/box-shadow|focus-within|textarea|field-sizing/);
    expect(css).not.toContain('__files a');
    const markup = String(
      AIConversation({ open: true, tool: 'Codex', messages: [], draft: '', busy: false, interruptible: false }),
    );
    expect(markup).toContain('data-footer-presentation="floating"');
    expect(markup).toContain('data-body-layout="fill-last"');
  });
  it('sizes its dialog panel through Kerf DialogSurface with an 8px viewport gutter (HS2-MWMMCN, HS2-ADM62E)', () => {
    const markup = String(
      AIConversation({ open: true, tool: 'Codex', messages: [], draft: '', busy: false, interruptible: false }),
    );
    expect(markup).toMatch(/data-component="dialog-surface"/);
    expect(markup).toMatch(/data-preferred-width/);
    expect(markup).toMatch(/data-viewport-gutter/);
    expect(markup).toMatch(/data-body-inset="none"/);
    expect(css).not.toMatch(/::part\(dialog\) \{[^}]*width:/);
  });
  it('scrolls a foreground taller than the dialog in the foreground itself, keeping the shadow room (HS2-VYM95K)', () => {
    expect(css).toMatch(
      /\.ai-conversation__foreground \{[^}]*padding: remify\(96px\) remify\(32px\) remify\(16px\);[^}]*overflow-y: auto;[^}]*overscroll-behavior: contain;[^}]*pointer-events: none;/,
    );
    // The popup never becomes its own scroller, which clipped the card's shadow (HS2-SH3DR7).
    expect(css).not.toContain('.permission-request-popup');
  });
  it('offers exports for completed transcripts and makes partial saved transcripts read-only', () => {
    const message = {
      id: 'answer',
      role: 'assistant' as const,
      content: 'Saved result.',
      status: 'completed' as const,
    };
    const active = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [message],
        draft: 'Continue',
        busy: false,
        interruptible: false,
      }),
    );
    expect(active).toContain('data-action="save-conversation"');
    expect(active).toContain('aria-label="Save Conversation"');
    expect(active).toContain('send-conversation-turn');
    const saved = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [message],
        draft: '',
        busy: false,
        interruptible: false,
        readOnly: true,
        savedSource: '/Exports/review.hotsheet-chat',
      }),
    );
    expect(saved).toContain('data-read-only="true"');
    expect(saved).toContain('Saved transcript');
    expect(saved).toContain('/Exports/review.hotsheet-chat');
    expect(saved).not.toContain('send-conversation-turn');
    expect(saved).not.toContain('conversation-model');
  });
  it('keeps transcript messages selectable and exposes copy and clear actions for a range', () => {
    const messages = [
        { id: 'request', role: 'user' as const, content: 'Inspect this.' },
        { id: 'answer', role: 'assistant' as const, content: 'Done.', status: 'completed' as const },
      ],
      markup = String(
        AIConversation({
          open: true,
          presentation: 'embedded',
          tool: 'Codex',
          selectionId: 'connection-1',
          selectedMessageIds: ['request', 'answer'],
          messages,
          draft: '',
          busy: false,
          interruptible: false,
        }),
      );
    expect(markup).toContain('data-selection-id="connection-1"');
    expect(markup.match(/data-action="pick-conversation-message"/g)).toHaveLength(2);
    expect(markup.match(/data-selected="true"/g)).toHaveLength(2);
    expect(markup).toContain('2 messages selected');
    expect(markup).toContain('data-action="copy-conversation-selection"');
    expect(markup).toContain('data-action="clear-conversation-selection"');
  });
  it('renders live read-only previews without saved-transcript or mutation affordances', () => {
    const markup = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [{ id: 'answer', role: 'assistant', content: 'Previewed result.', status: 'completed' }],
        draft: '',
        busy: false,
        interruptible: false,
        readOnly: true,
        readOnlyContext: 'preview',
      }),
    );
    expect(markup).toContain('data-read-only="true"');
    expect(markup).toContain('Read-only preview');
    expect(markup).toContain('Return to the AI chat to continue this conversation.');
    expect(markup).toContain('Previewed result.');
    expect(markup).not.toContain('Saved transcript');
    expect(markup).not.toContain('save-conversation');
    expect(markup).not.toContain('send-conversation-turn');
  });
  it('promotes a permission popup into the dialog top layer without duplicating its inline card', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: true,
        interruptible: true,
        permissions: [permission],
        foreground: PermissionRequestPopup({ item: permission, layer: 'flow' }),
      }),
    );
    expect(markup).toContain('ai-conversation__foreground');
    // The foreground lays out an in-flow popup through the popup's own layer variant (HS2-M2W2DP).
    expect(markup).toContain('data-layer="flow"');
    expect(css).not.toContain('.permission-request-popup');
    expect(markup.match(/data-component="permission-request-card"/g)).toHaveLength(1);
  });
  it('shows the current model/effort with a popup to change them, including custom and Other, only when the plugin declares support', () => {
    const markup = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        selectionId: 'chat-one',
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: false,
        interruptible: false,
        model: 'legacy model',
        effort: 'high',
        models: [{ id: 'gpt', label: 'GPT' }],
        efforts: ['medium', 'high'],
        canChangeModel: true,
        canChangeEffort: true,
      }),
    );
    expect(markup).toContain('data-component="conversation-model-control"');
    // Current model + effort shown as a compact label.
    expect(markup).toContain('class="ai-conversation__model-name" title="legacy model">legacy model');
    expect(markup).toMatch(
      /ai-conversation__model-effort" data-component="badge" data-tone="neutral"[^>]*aria-label="Effort high">high</,
    );
    // Popup lets you pick a listed model, the current custom model, Other…, and an effort.
    expect(markup).toContain('data-value="gpt" slot="submenu" data-action="select-conversation-model" value="gpt"');
    expect(markup).toContain(
      'data-value="legacy model" slot="submenu" data-action="select-conversation-model" value="legacy model"',
    );
    expect(markup).toContain('data-action="open-conversation-manual-model"');
    expect(markup).toContain('data-value="high" slot="submenu" data-action="select-conversation-effort" value="high"');
    expect(markup).not.toContain('data-action="stop-conversation"');
  });
  it('omits the model popup when the plugin declares no model/effort support', () => {
    const markup = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: false,
        interruptible: false,
        model: 'gpt',
        models: [{ id: 'gpt', label: 'GPT' }],
      }),
    );
    expect(markup).not.toContain('data-component="conversation-model-control"');
  });
  it('offers a provider submenu that lists other providers when more than one is configured (HS2-PRBGRB)', () => {
    const markup = String(
      AIConversation({
        open: true,
        presentation: 'embedded',
        selectionId: 'chat-one',
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: false,
        interruptible: false,
        providerId: 'codex',
        providers: [
          { id: 'codex', label: 'Codex' },
          { id: 'claude', label: 'Claude' },
        ],
        canChangeProvider: true,
      }),
    );
    // The popup renders even with no model/effort support because provider change is available.
    expect(markup).toContain('data-component="conversation-model-control"');
    expect(markup).toContain(
      'data-value="claude" slot="submenu" data-action="select-conversation-provider" value="claude"',
    );
    // The current provider is marked selected.
    // The current provider is a checked PopupMenu choice (HS2-CSRJ9Y).
    expect(markup).toMatch(
      /data-value="codex" slot="submenu" data-action="select-conversation-provider" value="codex" type="checkbox" checked/,
    );
  });
  it('hides the provider submenu for read-only chats or a single configured provider (HS2-PRBGRB)', () => {
    expect(
      String(
        AIConversation({
          open: true,
          presentation: 'embedded',
          tool: 'Codex',
          messages: [],
          draft: '',
          busy: false,
          interruptible: false,
          providerId: 'codex',
          providers: [{ id: 'codex', label: 'Codex' }],
          canChangeProvider: true,
        }),
      ),
    ).not.toContain('data-action="select-conversation-provider"');
    expect(
      String(
        AIConversation({
          open: true,
          presentation: 'embedded',
          tool: 'Codex',
          messages: [{ id: 'm', role: 'user', content: 'hi' }],
          draft: '',
          busy: false,
          interruptible: false,
          readOnly: true,
          providerId: 'codex',
          providers: [
            { id: 'codex', label: 'Codex' },
            { id: 'claude', label: 'Claude' },
          ],
          canChangeProvider: true,
        }),
      ),
    ).not.toContain('data-action="select-conversation-provider"');
  });
  it('distinguishes the conversation lifecycle from a nested model popup closing', () => {
    const surface = {} as Element,
      select = {} as Element;
    expect(isConversationSurfaceLifecycleEvent({ target: select }, surface)).toBe(false);
    expect(isConversationSurfaceLifecycleEvent({ target: surface }, surface)).toBe(true);
  });
  it('keeps nested Markdown and usage legible on the loud user bubble', () => {
    const markup = String(
      AIConversation({
        open: true,
        tool: 'Codex',
        messages: [
          { id: 'question', role: 'user', content: 'See [the spec](https://example.com).', sequence: 0 },
          { id: 'answer', role: 'assistant', content: 'Read it.', status: 'completed', sequence: 1 },
        ],
        draft: '',
        busy: false,
        interruptible: false,
      }),
    );
    // The user bubble selects the inverse MarkdownPreview tone (text and links take the loud bubble's
    // color); assistant responses inherit the bubble color and keep link color (HS2-7RY5GK).
    expect(markup).toMatch(/data-component="markdown-preview" data-tone="inverse" data-density="compact"/);
    expect(markup).toMatch(/data-component="markdown-preview" data-tone="inherit" data-density="compact"/);
    // Messages are Kerf ContentItem chat bubbles: the person's on brand-loud at the end (HS2-P16J04).
    expect(markup).toMatch(/data-component="content-item" data-appearance="brand-loud" data-bubble="end"/);
    expect(markup).toMatch(/data-component="content-item" data-appearance="neutral" data-bubble="start"/);
    const assistant = markup.slice(markup.indexOf('ai-conversation__message--assistant'));
    expect(assistant).not.toContain('ai-conversation__message-author');
    expect(assistant).not.toContain('data-component="ai-content-label"');
    expect(assistant).toContain('aria-label="AI-generated response by Codex"');
    expect(previewCss).toMatchSource(
      /\.markdown-preview\[data-tone='inverse'\] a \{[^}]*color:inherit[^}]*text-decoration-color:currentcolor/,
    );
    expect(css).toMatch(/\.ai-conversation__usage \{[^}]*color: color-mix\(in srgb, currentcolor/);
  });
});

describe('AI conversation foreground pointer interactivity (HS2-Q5TRYD)', () => {
  it('becomes pointer-interactive only while its scroll timeline is active (it overflows)', () => {
    const source = css,
      rule = source.slice(source.indexOf('.ai-conversation__foreground {'), source.indexOf('@keyframes'));
    expect(rule).toContain('pointer-events: none;');
    expect(rule).toContain('animation: ai-conversation-foreground-scrollable linear both;');
    expect(rule).toContain('animation-timeline: scroll(self);');
    expect(source).toMatch(
      /@keyframes ai-conversation-foreground-scrollable \{\s*from,\s*to \{\s*pointer-events: auto;\s*\}\s*\}/,
    );
  });
});
