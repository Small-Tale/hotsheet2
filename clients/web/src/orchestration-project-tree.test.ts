import { describe, expect, it } from 'vitest';

import {
  expandedProjectGroups,
  flattenProjectTree,
  groupStatus,
  isProjectGroup,
  PROJECT_COLLAPSE_STORAGE_KEY,
  projectCollapseKey,
  projectGroupId,
  projectTree,
  readCollapsedProjectGroups,
  selectedProjectNode,
  siteSummary,
  targetOfNode,
  withProjectGroupCollapsed,
} from './orchestration-project-tree';
import type { ConnectionStatus, OrchestrationProjectRow } from './orchestration-settings-model';

const BOUND: ConnectionStatus = { label: 'Bound', tone: 'neutral' };
const row = (
  targetId: string,
  machine: string | undefined,
  options: Partial<OrchestrationProjectRow> = {},
): OrchestrationProjectRow => ({
  targetId,
  label: targetId,
  location: 'local',
  where: `Local on ${machine ?? ''}`,
  place: machine ? `Local on ${machine}` : '',
  machine,
  connection: BOUND,
  ...options,
});

class MemoryStorage {
  values = new Map<string, string>();
  writes = 0;
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.writes += 1;
    this.values.set(key, value);
  }
}

describe('the Projects tree (HS2-M1T6KN)', () => {
  it('makes a project with several sites a group of its home site then its sites, and keeps one site a leaf', () => {
    const tree = projectTree([
      row('API', 'This Computer', { where: '3 sites · Local on This Computer', system: 'macOS 27.2, arm64' }),
      row('Build · API', 'Build Host', { siteOf: 'API', place: 'Remote on Build Host', location: 'remote' }),
      row('Studio · API', 'Studio Mac', {
        siteOf: 'API',
        place: 'Remote on Studio Mac',
        connection: { label: 'Unreachable', tone: 'danger' },
      }),
      row('Docs', 'This Computer'),
    ]);
    expect(tree.map((node) => [node.id, node.kind])).toEqual([
      ['project:API', 'project'],
      ['Docs', 'single'],
    ]);
    const [api, docs] = tree;
    expect(api.label).toBe('API');
    expect(api.description).toBe('3 sites · This Computer, Build Host, Studio Mac');
    expect(api.status).toEqual({ label: '1 Unreachable', tone: 'danger' });
    expect(api.children?.map((node) => [node.id, node.kind, node.description, node.status?.label])).toEqual([
      ['API', 'home', 'Home Site · Local on This Computer · macOS 27.2, arm64', 'Bound'],
      ['Build · API', 'site', 'Site · Remote on Build Host', 'Bound'],
      ['Studio · API', 'site', 'Site · Remote on Studio Mac', 'Unreachable'],
    ]);
    expect(docs.description).toBe('Local on This Computer');
    expect(docs.children).toBeUndefined();
    expect(flattenProjectTree(tree).map((node) => node.id)).toEqual([
      'project:API',
      'API',
      'Build · API',
      'Studio · API',
      'Docs',
    ]);
  });

  it('reads an empty list as no nodes, and a site whose project is gone as a leaf', () => {
    expect(projectTree([])).toEqual([]);
    expect(projectTree([row('Orphan', 'Build Host', { siteOf: 'Gone' })]).map((node) => node.kind)).toEqual(['single']);
  });

  it('names up to three machines, then the first three and how many more', () => {
    const sites = (machines: (string | undefined)[]) =>
      machines.map((machine, index) => row(`S${String(index)}`, machine));
    expect(siteSummary(sites(['This Computer', 'Build Host']))).toBe('2 sites · This Computer, Build Host');
    expect(siteSummary(sites(['This Computer', 'Build Host', 'Studio Mac']))).toBe(
      '3 sites · This Computer, Build Host, Studio Mac',
    );
    expect(siteSummary(sites(['A', 'B', 'C', 'D']))).toBe('4 sites · A, B, C and 1 more');
    expect(siteSummary(sites(['A', 'B', 'C', 'D', 'E']))).toBe('5 sites · A, B, C and 2 more');
    // One machine with two sites is named once; a site not bound here says so; an unread binding is left out.
    expect(siteSummary(sites(['This Computer', 'This Computer', '', undefined]))).toBe(
      '4 sites · This Computer, Not Bound',
    );
    expect(siteSummary(sites([undefined, undefined]))).toBe('2 sites');
  });

  it('rolls the sites up to the most severe state, or All <state> when they agree', () => {
    const status = (label: string, tone: ConnectionStatus['tone']) => row(label, 'M', { connection: { label, tone } });
    expect(groupStatus([status('Bound', 'neutral'), status('Bound', 'neutral')])).toEqual({
      label: 'All Bound',
      tone: 'neutral',
    });
    expect(
      groupStatus([status('Not Bound', 'warning'), status('Unreachable', 'danger'), status('Unreachable', 'danger')]),
    ).toEqual({ label: '2 Unreachable', tone: 'danger' });
    expect(groupStatus([status('Bound', 'neutral'), status('Not Bound', 'warning')])).toEqual({
      label: '1 Not Bound',
      tone: 'warning',
    });
    expect(groupStatus([status('Bound', 'neutral'), status('Checking', 'neutral')])).toBeUndefined();
    expect(groupStatus([])).toBeUndefined();
  });

  it('maps a group to its home site, and keeps a chosen group selected only while its home is the target', () => {
    expect(projectGroupId('T1')).toBe('project:T1');
    expect(targetOfNode('project:T1')).toBe('T1');
    expect(targetOfNode('T1')).toBe('T1');
    expect(isProjectGroup('project:T1')).toBe(true);
    expect(isProjectGroup('T1')).toBe(false);
    expect(selectedProjectNode(undefined, 'project:T1')).toBeUndefined();
    expect(selectedProjectNode('T1', 'project:T1')).toBe('project:T1');
    expect(selectedProjectNode('T1', 'T1')).toBe('T1');
    expect(selectedProjectNode('T2', 'project:T1')).toBe('T2');
    expect(selectedProjectNode('T2')).toBe('T2');
  });
});

describe('remembered project groups (HS2-M1T6KN)', () => {
  const tree = projectTree([
    row('API', 'A'),
    row('API site', 'B', { siteOf: 'API' }),
    row('Web', 'A'),
    row('Web site', 'C', { siteOf: 'Web' }),
    row('Docs', 'A'),
  ]);

  it('opens every group by default, per orchestration project', () => {
    expect(expandedProjectGroups(tree, 'owner', new Set())).toEqual(['project:API', 'project:Web']);
    const closed = new Set([projectCollapseKey('owner', 'API')]);
    expect(expandedProjectGroups(tree, 'owner', closed)).toEqual(['project:Web']);
    // Another orchestration project's preference does not apply.
    expect(expandedProjectGroups(tree, 'other', closed)).toEqual(['project:API', 'project:Web']);
  });

  it('closes, reopens, closes again and survives a reload through storage', () => {
    const storage = new MemoryStorage(),
      key = projectCollapseKey('owner', 'API');
    let collapsed = readCollapsedProjectGroups(storage);
    expect(collapsed.size).toBe(0);
    collapsed = withProjectGroupCollapsed(collapsed, key, true, storage);
    expect(readCollapsedProjectGroups(storage).has(key)).toBe(true);
    collapsed = withProjectGroupCollapsed(collapsed, key, false, storage);
    expect(readCollapsedProjectGroups(storage).has(key)).toBe(false);
    collapsed = withProjectGroupCollapsed(collapsed, key, true, storage);
    collapsed = withProjectGroupCollapsed(collapsed, projectCollapseKey('owner', 'Web'), true, storage);
    // A reload reads what was written.
    expect(expandedProjectGroups(tree, 'owner', readCollapsedProjectGroups(storage))).toEqual([]);
    // Setting the state it already has writes nothing and keeps the same set.
    const writes = storage.writes;
    expect(withProjectGroupCollapsed(collapsed, key, true, storage)).toBe(collapsed);
    expect(storage.writes).toBe(writes);
  });

  it('reads missing, malformed or unreadable storage as every group open, and survives a failing write', () => {
    expect(readCollapsedProjectGroups(undefined).size).toBe(0);
    const storage = new MemoryStorage();
    storage.values.set(PROJECT_COLLAPSE_STORAGE_KEY, '{not json');
    expect(readCollapsedProjectGroups(storage).size).toBe(0);
    storage.values.set(PROJECT_COLLAPSE_STORAGE_KEY, JSON.stringify({ owner: true }));
    expect(readCollapsedProjectGroups(storage).size).toBe(0);
    storage.values.set(PROJECT_COLLAPSE_STORAGE_KEY, JSON.stringify(['a', 1, null, 'b']));
    expect([...readCollapsedProjectGroups(storage)]).toEqual(['a', 'b']);
    const failing = {
      setItem: () => {
        throw new Error('quota');
      },
    };
    expect([...withProjectGroupCollapsed(new Set(), 'k', true, failing)]).toEqual(['k']);
    expect([...withProjectGroupCollapsed(new Set(), 'k', true, undefined)]).toEqual(['k']);
  });
});
