import { describe, expect, it } from 'vitest';

import type { OrchestrationRoleSession } from '../api';
import { OrchestrationProjectDetail } from './orchestration-project-detail';

const toolLabel = (tool: string) => (tool === 'claude' ? 'Claude' : tool);

const render = (props: Partial<Parameters<typeof OrchestrationProjectDetail>[0]> = {}) =>
  String(OrchestrationProjectDetail({ targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', ...props }));

describe('OrchestrationProjectDetail keeps only this project’s admission (HS2-JF3EA3)', () => {
  const text = (markup: string, label: string) =>
    (new RegExp(`aria-label="${label}"[\\s\\S]*?</section>`).exec(markup)?.[0] ?? '')
      .replace(/<[^>]+>/g, ' ')
      .replace(/\s+/g, ' ');
  const detail = {
    targetId: 'W1',
    loading: false,
    admissions: [
      { outcome: 'accepted' as const, tool: 'claude', requested: 3, admitted: 3, reason: null },
      { outcome: 'reduced' as const, tool: 'codex', requested: 2, admitted: 1, reason: 'session_cap_reached' },
      { outcome: 'refused' as const, tool: 'gemini', requested: 1, admitted: 0, reason: 'below_ram_floor' },
    ],
  };

  it('shows each tool’s admission line and moves the machine sections to Systems', () => {
    const markup = render({
      toolLabel,
      detail,
      systemsMachineId: 'local',
      sites: [
        { targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', home: true },
        { targetId: 'W2', label: 'Widgets build host', where: 'Remote on Build host', home: false },
      ],
    });
    const admission = text(markup, 'Admission');
    expect(admission).toContain('Claude Can start 3 now');
    expect(admission).toContain('codex Can start 1 of 2 · session cap reached');
    expect(admission).toContain('gemini Refused · memory below the floor');
    for (const gone of [
      'Capacity and Admission',
      'aria-label="Sites"',
      'AI Tool Capacity',
      'Free Memory',
      'Toolchains',
    ])
      expect(markup).not.toContain(gone);
    // Each site's place stays in Binding.
    expect(text(markup, 'Binding')).toContain('Remote on Build host');
  });

  it('opens Systems on the project’s machine, or on its current machine when unknown', () => {
    const named = render({ detail, systemsMachineId: 'remote:01ARZ3NDEKTSV4RRFFQ69G5FAV' });
    expect(named).toMatch(
      /data-machine-id="remote:01ARZ3NDEKTSV4RRFFQ69G5FAV"[^>]*data-action="select-systems-machine"|data-action="select-systems-machine"[^>]*data-machine-id="remote:01ARZ3NDEKTSV4RRFFQ69G5FAV"/u,
    );
    expect(named).toContain('Open in Systems');
    const unknown = render({ detail });
    expect(unknown).toContain('data-action="select-systems-machine"');
    expect(unknown).not.toContain('data-machine-id');
  });

  it('says when no tool is allowed, when admission failed, and waits while loading', () => {
    expect(text(render({ detail: { ...detail, admissions: [], admissionTools: [] } }), 'Admission')).toContain(
      'No AI tool is allowed here yet.',
    );
    expect(text(render({ detail: { ...detail, admissions: [], admissionError: 'refused' } }), 'Admission')).toContain(
      'Admission is not available. refused',
    );
    expect(render({ detail: { targetId: 'W1', admissions: [], loading: true } })).toMatch(
      /aria-label="Admission"[\s\S]*data-placeholder/u,
    );
  });
});

/** The markup of one section, by its aria-label. */
const sectionOf = (markup: string, label: string) =>
  new RegExp(`aria-label="${label}"[\\s\\S]*?</section>`).exec(markup)?.[0] ?? '';
const textOf = (markup: string) => markup.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ');

describe('OrchestrationProjectDetail Binding names every site (HS2-NGP2AK)', () => {
  it('lists each site on its own row, the home site marked, when the project has several', () => {
    const binding = textOf(
      sectionOf(
        render({
          where: '2 sites · Local on This Computer, Remote on Build host',
          sites: [
            { targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', home: true },
            { targetId: 'W2', label: 'Widgets build host', where: 'Remote on Build host', home: false },
            { targetId: 'W3', label: 'Widgets spare', where: '', home: false },
          ],
        }),
        'Binding',
      ),
    );
    for (const text of [
      'Where 3 sites',
      'Widgets (Home) Local on This Computer',
      'Widgets build host Remote on Build host',
      'Widgets spare Not bound on this computer',
    ])
      expect(binding).toContain(text);
  });

  it('keeps one Where row for a one-site project', () => {
    const binding = textOf(sectionOf(render(), 'Binding'));
    expect(binding).toContain('Where Local on This Computer');
    expect(binding).not.toContain('sites');
  });
});

describe('OrchestrationProjectDetail Limits (HS2-Y737GM, HS2-25KT42, HS2-1X1TXC, HS2-NPAZQW)', () => {
  const limitsDetail = (overrides: Record<string, unknown> = {}) => ({
    targetId: 'W1',
    admissions: [],
    loading: false,
    limits: {
      targetId: 'W1',
      listed: true,
      enabled: true,
      tools: ['claude'],
      max_sessions: '',
      machineOverride: false,
      machine: { max_sessions: '2', min_free_ram_mb: '2048', max_cpu_percent: '90', min_free_disk_mb: '5120' },
      buildAhead: 'default' as const,
      models: { claude: 'opus-*' },
      resources: '',
      // HS2-R2N33M: unlisted coordination sessions are Auto, shown as both.
      roleSessions: ['dispatcher', 'integrator'] as OrchestrationRoleSession[],
      roleSessionsAuto: true,
      ...overrides,
    },
  });
  const limits = (props: Partial<Parameters<typeof OrchestrationProjectDetail>[0]> = {}) =>
    sectionOf(render({ toolLabel, policyTools: ['claude', 'codex'], detail: limitsDetail(), ...props }), 'Limits');

  it('offers the Limits help from its header', () => {
    const markup = limits();
    expect(markup).toContain('data-action="show-orchestration-limits-help"');
    expect(markup).toContain('About Limits');
    expect(markup).toContain('aria-haspopup="dialog"');
  });

  it('keeps the Auto limits closed under Advanced, with a summary of what changed', () => {
    const markup = limits();
    const advanced = /<details[^>]*data-orchestration-advanced-limits[\s\S]*<\/details>/.exec(markup)?.[0] ?? '';
    expect(advanced).not.toBe('');
    expect(advanced).not.toMatch(/<details[^>]*\sopen/u);
    expect(textOf(advanced)).toContain('All Auto');
    for (const name of ['limits.role_sessions', 'limits.max_sessions', 'limits.build_ahead', 'limits.machine_override'])
      expect(advanced).toContain(`name="${name}"`);
    expect(advanced).toContain('data-limits-field="limits.resources"');
    // The default view keeps only the switch, the tools and their models outside.
    const outside = markup.replace(advanced, '');
    expect(outside).toContain('name="limits.enabled"');
    expect(outside).toContain('name="limits.tools"');
    expect(outside).toContain('data-limits-field="limits.models.claude"');
    for (const name of ['limits.max_sessions', 'limits.build_ahead']) expect(outside).not.toContain(`name="${name}"`);
    // The hints say what each Auto means.
    expect(textOf(advanced)).toContain('The most AI sessions the orchestrator runs in this project at once');
    expect(textOf(advanced)).toContain('before a ticket it depends on has passed CI');
    const changed = limits({ detail: limitsDetail({ max_sessions: '1', resources: 'vm/*' }) });
    expect(textOf(changed)).toContain('2 Changed');
  });

  it('shows chosen models and resources as removable chips with known names and patterns to choose', () => {
    const markup = limits({
      toolModels: { claude: ['claude-opus-4-1', 'claude-sonnet-4-5'] },
      resourceChoices: ['vm/*', 'vm/mac-1'],
      detail: limitsDetail({ resources: 'vm/mac-1, ports/*' }),
    });
    const models =
      /data-limits-field="limits\.models\.claude"[\s\S]*?(?=data-component="orchestration-pattern-field"|$)/.exec(
        markup,
      )?.[0];
    expect(models).toContain('data-item-id="opus-*"');
    expect(models).toContain('Remove opus-*');
    const resources = markup.slice(markup.indexOf('data-limits-field="limits.resources"'));
    expect(resources).toContain('data-item-id="vm/mac-1"');
    expect(resources).toContain('data-item-id="ports/*"');
    expect(resources).toContain('data-action="remove-orchestration-limits-value"');
    // A chosen value is not offered again.
    expect(markup).toContain('data-combobox-id="orchestration-limits-limits-resources"');
  });
});
