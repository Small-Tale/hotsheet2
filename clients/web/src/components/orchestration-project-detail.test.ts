import { describe, expect, it } from 'vitest';

import type { OrchestrationCapabilities } from '../api';
import { OrchestrationProjectDetail } from './orchestration-project-detail';

const macos: OrchestrationCapabilities = {
  os: 'macos',
  os_version: '15.1',
  arch: 'arm64',
  cpu_model: 'Synthetic 10-core CPU',
  gpu: { present: true, vendors: ['apple'] },
  ai_tools: [{ tool: 'claude', version: '2.0.14' }],
  toolchains: [{ name: 'node', installed: true, version: '22.11.0' }],
};
const linux: OrchestrationCapabilities = { os: 'linux', arch: 'x86_64' };
const toolLabel = (tool: string) => (tool === 'claude' ? 'Claude' : tool);

const render = (props: Partial<Parameters<typeof OrchestrationProjectDetail>[0]> = {}) =>
  String(OrchestrationProjectDetail({ targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', ...props }));

/** The text of one site's table, tags stripped. */
const siteText = (markup: string, target: string) =>
  (new RegExp(`data-orchestration-site="${target}"[\\s\\S]*?</dl>`).exec(markup)?.[0] ?? '').replace(/<[^>]+>/g, ' ');

describe('OrchestrationProjectDetail Sites (HS2-D4G81Q)', () => {
  it('shows every site of a project with several sites, with what each machine is', () => {
    const markup = render({
      toolLabel,
      sites: [
        { targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', home: true, capabilities: macos },
        {
          targetId: 'W2',
          label: 'Widgets build host',
          where: 'Remote on Build host',
          home: false,
          capabilities: linux,
        },
        {
          targetId: 'W3',
          label: 'Widgets laptop',
          where: 'Remote on Laptop',
          home: false,
          error: 'Cannot be reached now',
        },
        { targetId: 'W4', label: 'Widgets spare', where: '', home: false },
      ],
    });
    expect(markup).toContain('aria-label="Sites"');
    const home = siteText(markup, 'W1');
    for (const text of [
      'Widgets · Home',
      'Local on This Computer',
      'macOS 15.1, arm64',
      'Synthetic 10-core CPU',
      'Apple',
      'Claude 2.0.14',
      'node 22.11.0',
    ])
      expect(home).toContain(text);
    // A machine that reports little reads as unknown, never as absent; no Processor row without a model.
    const build = siteText(markup, 'W2');
    expect(build).toContain('Linux, x86_64');
    expect(build).not.toContain('Processor');
    expect(build).toContain('Unknown');
    // A failed read names why, and a site not bound here says so.
    expect(siteText(markup, 'W3')).toContain('Cannot be reached now');
    const spare = siteText(markup, 'W4');
    expect(spare).toContain('Not bound on this computer');
    expect(spare).toContain('Not bound here');
  });

  it('shows no Sites section for a one-site project', () => {
    expect(render()).not.toContain('aria-label="Sites"');
    expect(
      render({ sites: [{ targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', home: true }] }),
    ).not.toContain('aria-label="Sites"');
  });

  const capacityDetail = (capabilities: OrchestrationCapabilities) => ({
    targetId: 'W1',
    admissions: [],
    loading: false,
    capacity: {
      sampled_at_ms: 1,
      memory: { free_mb: 1, total_mb: 2 },
      cpu: { load_percent: 1, logical_cpus: 2 },
      disk: { free_mb: 1, total_mb: 2 },
      tools: null,
      sessions: null,
      claims: null,
      claim_namespaces: [],
      capabilities,
    },
  });
  /** The text of the capacity region, tags stripped. */
  const capacityText = (markup: string) =>
    (/aria-label="Capacity and Admission"[\s\S]*?<\/section>/.exec(markup)?.[0] ?? '').replace(/<[^>]+>/g, ' ');

  it('shows a one-site project machine rows in the capacity region (HS2-ZY4Z4G)', () => {
    const capacity = capacityText(render({ toolLabel, detail: capacityDetail(macos) }));
    for (const label of ['System', 'Processor', 'GPU', 'AI Tools', 'Toolchains']) expect(capacity).toContain(label);
    expect(capacity).toContain('macOS 15.1, arm64');
  });

  it('shows the machine rows of a project with several sites only in Sites, from the fresh read for this site', () => {
    const markup = render({
      toolLabel,
      // The capacity read is newer than the sites view's for this site.
      detail: capacityDetail({ ...macos, os_version: '15.2' }),
      sites: [
        { targetId: 'W1', label: 'Widgets', where: 'Local on This Computer', home: true, capabilities: macos },
        {
          targetId: 'W2',
          label: 'Widgets build host',
          where: 'Remote on Build host',
          home: false,
          capabilities: linux,
        },
      ],
    });
    const capacity = capacityText(markup);
    expect(capacity).toContain('Free Memory');
    for (const label of ['System', 'Processor', 'GPU', 'AI Tools', 'Toolchains']) expect(capacity).not.toContain(label);
    expect(siteText(markup, 'W1')).toContain('macOS 15.2, arm64');
    expect(siteText(markup, 'W2')).toContain('Linux, x86_64');
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
      roleSessions: [],
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
