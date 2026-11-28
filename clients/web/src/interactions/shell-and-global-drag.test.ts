import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ticketDropTarget } from '../ticket-drop-target';
import { type ShellAndGlobalInteractionsDependencies, wireShellAndGlobalInteractions } from './shell-and-global';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
  delegateCapture: () => () => undefined,
}));
vi.mock('../components/context-menu-dismissal', () => ({ wireContextMenuDismissal: () => () => undefined }));

const STATUS_TARGETS = '[data-ticket-drop-status], [data-ticket-drop-action], [data-ticket-drop-project]';
const UNGROUP_TARGET = '[data-ticket-drop-ungroup="true"]';
const source = { id: 'local', name: 'Local' } as never;

/** A board column, or a child of one, as far as the drag handlers read it. */
function element(dataset: Record<string, string>, parent?: FakeElement): FakeElement {
  const node: FakeElement = {
    dataset,
    parent,
    closest(selector: string) {
      for (let at: FakeElement | undefined = node; at; at = at.parent)
        if (selector.includes('ticket-drop-group') && at.dataset.ticketDropGroup !== undefined) return at;
      return null;
    },
    matches: () => false,
    contains: () => true,
  };
  return node;
}
interface FakeElement {
  dataset: Record<string, string | undefined>;
  parent?: FakeElement;
  closest: (selector: string) => FakeElement | null;
  matches: (selector: string) => boolean;
  contains: (other: unknown) => boolean;
}
function dragEvent(type: string, target: FakeElement) {
  const transfer = { dropEffect: 'none' };
  return { type, target, dataTransfer: transfer, defaultPrevented: false, preventDefault: vi.fn() };
}
function handlers(event: string, selector: string) {
  return registrations.filter((item) => item.event === event && item.selector === selector);
}

describe('ticket drop targets accept on dragenter (HS2-SSD28G)', () => {
  let dependencies: ShellAndGlobalInteractionsDependencies;
  beforeEach(() => {
    registrations.length = 0;
    ticketDropTarget.value = undefined;
    vi.stubGlobal('window', { addEventListener: vi.fn(), matchMedia: () => ({ matches: false }) });
    vi.stubGlobal('document', { body: {}, addEventListener: vi.fn() });
    const known: Record<string, unknown> = {
      draggedTickets: undefined,
      project: () => source,
      projects: signal([source]),
      tickets: signal([{ slug: 'HS2-A', status: 'not_started', group: 'G1' }]),
      selectedTicketSlugs: signal([]),
      ticketGroups: { disabledReason: () => undefined, setGroup: vi.fn() },
    };
    // Every other dependency is only read when its own interaction runs.
    dependencies = new Proxy(known, {
      get: (target, key: string) => (key in target ? target[key] : Object.assign(vi.fn(), { value: undefined })),
      set: (target, key: string, value) => {
        target[key] = value;
        return true;
      },
    }) as unknown as ShellAndGlobalInteractionsDependencies;
    wireShellAndGlobalInteractions(dependencies);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('registers dragenter beside dragover for the status, action and project targets and the ungroup zone', () => {
    expect(handlers('dragenter', STATUS_TARGETS)).toHaveLength(1);
    expect(handlers('dragover', STATUS_TARGETS)).toHaveLength(1);
    expect(handlers('dragenter', UNGROUP_TARGET)).toHaveLength(1);
    expect(handlers('dragover', UNGROUP_TARGET)).toHaveLength(1);
  });

  it('accepts a move into another column on the enter alone, the move WebKit sends onto a child', () => {
    const column = element({ ticketDropStatus: 'started', ticketDropKey: 'column:started' }),
      label = element({}, column);
    const [enter] = handlers('dragenter', STATUS_TARGETS);
    // No drag in progress: the enter is not accepted.
    const idle = dragEvent('dragenter', label);
    enter.handle(idle as never, column as never);
    expect(idle.preventDefault).not.toHaveBeenCalled();
    expect(ticketDropTarget.value).toBeUndefined();

    dependencies.draggedTickets = { slugs: ['HS2-A'], source };
    const crossing = dragEvent('dragenter', label);
    enter.handle(crossing as never, column as never);
    expect(crossing.preventDefault).toHaveBeenCalledOnce();
    expect(crossing.dataTransfer.dropEffect).toBe('move');
    expect(ticketDropTarget.value).toBe('column:started');
    // The next dragover keeps the same answer.
    const over = dragEvent('dragover', label);
    handlers('dragover', STATUS_TARGETS)[0].handle(over as never, column as never);
    expect(over.preventDefault).toHaveBeenCalledOnce();
    expect(ticketDropTarget.value).toBe('column:started');
  });

  it('accepts entering the ungroup zone only for a grouped ticket outside every group', () => {
    const zone = element({ ticketDropUngroup: 'true' }),
      [enter] = handlers('dragenter', UNGROUP_TARGET);
    dependencies.draggedTickets = { slugs: ['HS2-A'], source };
    const entering = dragEvent('dragenter', zone);
    enter.handle(entering as never, zone as never);
    expect(entering.preventDefault).toHaveBeenCalledOnce();
    expect(entering.dataTransfer.dropEffect).toBe('move');
    // Over a group inside the zone, the group takes the drop instead.
    const group = element({ ticketDropGroup: 'G2' }, zone),
      overGroup = dragEvent('dragenter', group);
    enter.handle(overGroup as never, zone as never);
    expect(overGroup.preventDefault).not.toHaveBeenCalled();
  });
});
