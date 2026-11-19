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
