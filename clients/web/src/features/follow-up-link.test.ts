import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketRows, CodeReview, FullTicket, TicketRow } from '../api';
import type { Project } from '../interactions/types';
import { createFollowUpLinkController } from './follow-up-link';

const calls = vi.hoisted(() => ({
  search: [] as { apiPath: string; checkout: string; text: string }[],
  link: [] as { apiPath: string; checkout: string; source: string; target: string }[],
  rows: new Map<string, unknown[]>(),
  failLink: undefined as Error | undefined,
}));

vi.mock('../api', async (original) => {
  const actual = await original<typeof import('../api')>();
  return {
    ...actual,
    Api: class extends actual.Api {
      readonly path: string;
      constructor(path: string) {
        super(path);
        this.path = path;
      }
      override checkoutTickets = async (checkout: string, query?: unknown) => {
        calls.search.push({ apiPath: this.path, checkout, text: (query as { text: string }).text });
        return { items: calls.rows.get(checkout) ?? [], partial: false, truncated: false } as CheckoutTicketRows;
      };
      override linkCheckoutFollowUp = async (checkout: string, source: string, target: string) => {
        calls.link.push({ apiPath: this.path, checkout, source, target });
        if (calls.failLink) throw calls.failLink;
        return undefined;
      };
    },
  };
});

const project = (id: string) => ({ id, name: id.toUpperCase(), apiPath: `/__hotsheet/project-api/${id}` }) as Project;
const row = (connection: string, native: string, slug: string) =>
  ({
    id: native,
    slug,
    title: `Synthetic ${slug}`,
    connection_id: connection,
    native_id: native,
    qualified_id: `${connection}:${native}`,
  }) as TicketRow;

function setup(review?: CodeReview) {
  const alpha = project('alpha'),
    beta = project('beta'),
    projects = signal<Project[]>([alpha, beta]),
    selectedTicket = signal<FullTicket | null>(row('git-a', 'src', 'HS2-SRC') as FullTicket),
    codeReview = signal<CodeReview | undefined>(review),
    refreshCodeReview = vi.fn(async () => undefined),
    showToast = vi.fn();
  const controller = createFollowUpLinkController({
    project: () => alpha,
    projects,
    selectedTicket,
    codeReview,
    refreshCodeReview,
    showToast,
    searchDelayMs: 0,
  });
  return { controller, selectedTicket, refreshCodeReview, showToast };
}
const settle = async () => {
  await vi.runAllTimersAsync();
};

describe('createFollowUpLinkController (HS2-MCGCPQ)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    calls.search.length = 0;
    calls.link.length = 0;
    calls.failLink = undefined;
    calls.rows = new Map([
      ['alpha', [row('git-a', 'src', 'HS2-SRC'), row('git-a', 'one', 'HS2-ONE')]],
      ['beta', [row('git-b', 'two', 'HS2-TWO')]],
    ]);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('opens for the inspected ticket, records existing follow-ups, and refuses without one', () => {
    const { controller, selectedTicket } = setup({
      commits: [],
      ranges: [],
      truncated: false,
      follow_ups: [{ target: { project_id: 'beta', connection_id: 'git-b', native_id: 'two' }, state: 'resolved' }],
    });
    expect(controller.open()).toBe(true);
    expect(controller.dialog.value).toMatchObject({
      source: { projectId: 'alpha', qualifiedId: 'git-a:src', slug: 'HS2-SRC' },
      existing: ['beta::git-b%3Atwo'],
      query: '',
      candidates: [],
    });
    controller.close();
    expect(controller.dialog.value).toBeUndefined();
    selectedTicket.value = null;
    expect(controller.open()).toBe(false);
    expect(controller.dialog.value).toBeUndefined();
  });

  it('searches every open project, then selects, clears, and searches again', async () => {
    const { controller } = setup();
    controller.open();
    controller.search('hs2');
    expect(controller.dialog.value).toMatchObject({ query: 'hs2', searching: true, candidates: [] });
    await settle();
    expect(calls.search.map((call) => call.checkout)).toEqual(['alpha', 'beta']);
    expect(controller.dialog.value?.searching).toBe(false);
    expect(controller.dialog.value?.candidates.map((item) => item.slug)).toEqual(['HS2-SRC', 'HS2-ONE', 'HS2-TWO']);
    controller.select('beta::git-b%3Atwo');
    expect(controller.dialog.value).toMatchObject({ selected: { slug: 'HS2-TWO' }, query: '', candidates: [] });
    controller.select('missing');
    expect(controller.dialog.value?.selected?.slug).toBe('HS2-TWO');
    controller.clear();
    expect(controller.dialog.value?.selected).toBeUndefined();
    controller.search('');
    expect(controller.dialog.value).toMatchObject({ query: '', searching: false });
    await settle();
    expect(calls.search).toHaveLength(2);
  });

  it('drops a stale search when the person keeps typing or closes the dialog', async () => {
    const { controller } = setup();
    controller.open();
    controller.search('h');
    controller.search('hs2-two');
    await settle();
    expect(calls.search.map((call) => call.text)).toEqual(['hs2-two', 'hs2-two']);
    controller.search('again');
    controller.close();
    await settle();
    expect(calls.search).toHaveLength(2);
    expect(controller.dialog.value).toBeUndefined();
  });

  it('links through the target checkout with qualified references, then refreshes Code Review', async () => {
    const { controller, refreshCodeReview, showToast } = setup();
    controller.open();
    controller.search('two');
    await settle();
    controller.select('beta::git-b%3Atwo');
    await controller.submit();
    expect(calls.link).toEqual([
      {
        apiPath: '/__hotsheet/project-api/beta',
        checkout: 'beta',
        source: '@alpha/git-a:src',
        target: '@beta/git-b:two',
      },
    ]);
    expect(controller.dialog.value).toBeUndefined();
    expect(showToast).toHaveBeenCalledWith('HS2-TWO is now a follow-up of HS2-SRC.');
    expect(refreshCodeReview).toHaveBeenCalledTimes(1);
  });

  it('links an already-recorded follow-up idempotently and says so', async () => {
    const { controller, showToast } = setup({
      commits: [],
      ranges: [],
      truncated: false,
      follow_ups: [{ target: { project_id: 'alpha', connection_id: 'git-a', native_id: 'one' }, state: 'resolved' }],
    });
    for (let attempt = 0; attempt < 2; attempt += 1) {
      controller.open();
      controller.search('one');
      await settle();
      controller.select('alpha::git-a%3Aone');
      await controller.submit();
    }
    expect(calls.link).toHaveLength(2);
    expect(new Set(calls.link.map((call) => `${call.source}->${call.target}`)).size).toBe(1);
    expect(showToast).toHaveBeenLastCalledWith('HS2-ONE was already a follow-up of HS2-SRC.');
  });

  it('keeps the dialog open with the refusal, and blocks a self-link or an unselected submit', async () => {
    const { controller, refreshCodeReview, showToast } = setup();
    controller.open();
    await controller.submit();
    expect(controller.dialog.value?.error).toBe('Select the existing ticket to link as a follow-up.');
    controller.search('src');
    await settle();
    controller.select('alpha::git-a%3Asrc');
    await controller.submit();
    expect(controller.dialog.value?.error).toBe('A ticket cannot be its own follow-up.');
    controller.clear();
    controller.search('two');
    await settle();
    controller.select('beta::git-b%3Atwo');
    calls.failLink = new Error('follow-up relationships are not supported by this source provider');
    await controller.submit();
    expect(controller.dialog.value).toMatchObject({
      submitting: false,
      error: 'follow-up relationships are not supported by this source provider',
      selected: { slug: 'HS2-TWO' },
    });
    expect(showToast).not.toHaveBeenCalled();
    expect(refreshCodeReview).not.toHaveBeenCalled();
    // A retry after the refusal is corrected succeeds from the same dialog.
    calls.failLink = undefined;
    await controller.submit();
    expect(controller.dialog.value).toBeUndefined();
    expect(calls.link).toHaveLength(2);
  });
});
