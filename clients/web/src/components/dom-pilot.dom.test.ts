// HS2-3FQNGT: narrow, parent-composed DOM pilot; real browser contracts remain in Playwright.
import { afterEach, describe, expect, it } from 'vitest';

import type { TicketRow as WireTicketRow } from '../api';
import { insertSearchSyntax, wireTicketSearchFields } from '../interactions/ticket-search-field';
import { createTicketSearchModel } from '../ticket-search-model';
import type { WorkerView } from '../worker-view-model';
import type { TicketRowProps } from './ticket-row';
import { TicketSearchField } from './ticket-search-field';
import { WorkerColumns } from './worker-columns';

afterEach(() => {
  document.body.replaceChildren();
});

const mount = (markup: string) => {
  const host = document.createElement('main');
  host.innerHTML = markup;
  document.body.append(host);
  return host;
};

describe('HS2-3FQNGT DOM pilot', () => {
  it('composes search help through the field and delegated parent action', async () => {
    const model = createTicketSearchModel({ tags: () => ['client'] });
    const render = (helpOpen: boolean) =>
      String(TicketSearchField({ id: 'pilot-search', label: 'Search tickets', model, helpOpen }));
    const host = mount(render(false));
    const calls: string[] = [];
    let helpOpen = false;
    const dispose = wireTicketSearchFields(host, {
      applyDate: () => {},
      toggleHelp: (id) => {
        calls.push(`toggle:${id}`);
        helpOpen = !helpOpen;
        host.innerHTML = render(helpOpen);
      },
      insertSyntax: (id, token) => {
        calls.push(`insert:${id}:${token}`);
        insertSearchSyntax(model, token);
        helpOpen = false;
        host.innerHTML = render(false);
      },
      clear: () => {},
      editToken: () => {},
      removeToken: () => {},
    });
    try {
      const editor = host.querySelector<HTMLElement>('[data-search-field-editor]');
      expect(editor).not.toBeNull();
      editor?.focus();
      host.querySelector<HTMLButtonElement>('[data-action="toggle-ticket-search-help"]')?.click();
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(calls).toEqual(['toggle:pilot-search']);
      const chip = document.querySelector<HTMLButtonElement>('[data-item-id="is:duplicate"]');
      expect(chip).not.toBeNull();
      chip?.click();
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(calls).toContain('insert:pilot-search:is:duplicate');
      expect(model.state.value.tokens.some((item) => item.value === 'is:duplicate')).toBe(true);
      expect(host.querySelector('[data-search-field-editor]')).not.toBeNull();
    } finally {
      dispose();
    }
  });

  it('composes a Started-only Workers transition from server-shaped views and details', () => {
    const ticket: WireTicketRow = {
      connection_id: 'source',
      native_id: 'item',
      qualified_id: 'source:item',
      id: 'item',
      slug: 'HS2-DEMO',
      title: 'Synthetic ticket',
      category: 'task',
      priority: 'normal',
      status: 'started',
      up_next: true,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claim_count: 0,
    };
    const view: WorkerView = {
      cursor: 'c1',
      project_id: 'project',
      source_id: 'source',
      connection_coverage: true,
      uncovered_sources: [],
      workers: [
        {
          id: 'worker',
          role: 'worker',
          generation: 'g1',
          parent: null,
          label: 'Synthetic worker',
          tool: 'Codex',
          connection_kind: 'terminal',
          connection_id: 'source',
          connection_available: true,
          observed_alive: true,
          process_busy: false,
          availability: 'live',
          lease_expires_at: 1000,
          tickets: [
            {
              ticket: { connection_id: 'source', native_id: 'item' },
              slug: ticket.slug,
              assignment_id: 'assignment',
              assignment_phase: 'offered',
              claim_expires_at: null,
              started_phase: null,
              eta_at: null,
            },
          ],
          planned: [],
        },
      ],
    };
    const row = (item: WireTicketRow): TicketRowProps => ({
      slug: item.slug,
      title: item.title,
      status: 'started',
      priority: 'default',
      category: 'task',
      tags: [],
    });
    const render = (details: WireTicketRow) =>
      String(
        WorkerColumns({
          views: [view],
          details: { 'source:item': details },
          ticketRow: row,
          loading: false,
          sourceErrors: [],
          sources: [],
          layout: 'grid',
        }),
      );
    const host = mount(render(ticket));
    expect(host.querySelector('[data-worker-ticket="source:item"]')).not.toBeNull();
    expect(host.textContent).toContain('Offered · 1');
    host.innerHTML = render({ ...ticket, status: 'completed' });
    expect(host.querySelector('[data-worker-ticket="source:item"]')).toBeNull();
    expect(host.textContent).toContain('No assigned or planned tickets');
    expect(host.textContent).not.toContain('Offered · 1');
    host.innerHTML = render(ticket);
    expect(host.querySelector('[data-worker-ticket="source:item"]')).not.toBeNull();
  });

  it('probes custom elements, shadow DOM, focus, mutation delivery, event composition, and re-edit', async () => {
    class PilotField extends HTMLElement {
      constructor() {
        super();
        this.attachShadow({ mode: 'open' }).innerHTML = '<input aria-label="Pilot value">';
      }
    }
    if (!customElements.get('hs2-pilot-field')) customElements.define('hs2-pilot-field', PilotField);
    const host = mount('<hs2-pilot-field></hs2-pilot-field>');
    const field = host.querySelector<PilotField>('hs2-pilot-field');
    const input = field?.shadowRoot?.querySelector<HTMLInputElement>('input');
    expect(input).not.toBeNull();
    const mutations: MutationRecord[] = [];
    const observer = new MutationObserver((records) => mutations.push(...records));
    observer.observe(field!, { attributes: true });
    const seen: { event: Event; path: EventTarget[] }[] = [];
    host.addEventListener('input', (event) => seen.push({ event, path: event.composedPath() }));
    input!.focus();
    expect(field!.contains(document.activeElement) || document.activeElement === field).toBe(true);
    input!.value = 'first';
    input!.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    field!.setAttribute('data-value', 'first');
    await Promise.resolve();
    expect(mutations).toHaveLength(1);
    expect(seen).toHaveLength(1);
    expect(seen[0].path).toContain(input);
    input!.value = '';
    input!.value = 'again';
    input!.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    expect(seen).toHaveLength(2);
    expect(input!.value).toBe('again');
    observer.disconnect();
  });
});
