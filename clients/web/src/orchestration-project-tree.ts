/**
 * HS2-M1T6KN: Settings → Orchestration → Projects as a tree (docs/26 §26.25). A project with several sites is a
 * top-level node whose children are its home site and then its other sites; a project with one site is a leaf
 * at the top level. Pure: the component renders these nodes with Kerf `TreeList`, and the controller keeps the
 * per-browser collapse preference through the storage functions here.
 */
import type { ConnectionStatus, OrchestrationProjectRow } from './orchestration-settings-model';

/** A project group node's id prefix; the rest is the home site's target id. */
const GROUP_PREFIX = 'project:';
/** How many machines a collapsed summary names before "and N more". */
export const SUMMARY_MACHINES = 3;
/** A site whose machine is not bound here, in a summary. */
export const UNBOUND_MACHINE = 'Not Bound';

export type ProjectTreeNodeKind = 'project' | 'home' | 'site' | 'single';

/** One row of the Projects tree. */
export interface ProjectTreeNode {
  /** The TreeList node id: a target id, or `project:<home target id>` for a group. */
  id: string;
  kind: ProjectTreeNodeKind;
  /** The project or site row this node shows; a group shows its home site's row. */
  row: OrchestrationProjectRow;
  label: string;
  description?: string;
  /** The badge: a row's connection, or a group's roll-up (absent when its sites differ but none needs a look). */
  status?: ConnectionStatus;
  /** Home site, then the other sites by label; only on a group. */
  children?: readonly ProjectTreeNode[];
}

/** The group node id of the project whose home site is `homeTargetId`. */
export const projectGroupId = (homeTargetId: string) => `${GROUP_PREFIX}${homeTargetId}`;

/** The target a node opens in the inspector and acts on: a group stands for its home site. */
export function targetOfNode(nodeId: string): string {
  return nodeId.startsWith(GROUP_PREFIX) ? nodeId.slice(GROUP_PREFIX.length) : nodeId;
}

/** Whether `nodeId` is a project group rather than one site. */
export const isProjectGroup = (nodeId: string) => nodeId.startsWith(GROUP_PREFIX);

/**
 * The collapsed summary: the site count and each site's machine, once per machine, home site's first; with more
 * than three machines, the first three and "and N more" (the maintainer's request on HS2-M1T6KN).
 */
export function siteSummary(rows: readonly OrchestrationProjectRow[]): string {
  const machines = [
    ...new Set(
      rows.flatMap((row) => (row.machine === undefined ? [] : [row.machine === '' ? UNBOUND_MACHINE : row.machine])),
    ),
  ];
  const named =
    machines.length > SUMMARY_MACHINES
      ? `${machines.slice(0, SUMMARY_MACHINES).join(', ')} and ${String(machines.length - SUMMARY_MACHINES)} more`
      : machines.join(', ');
  return [`${String(rows.length)} sites`, named].filter(Boolean).join(' · ');
}

const SEVERITY: Readonly<Record<ConnectionStatus['tone'], number>> = { danger: 3, warning: 2, neutral: 1, success: 0 };

/**
 * A group's badge (Design v1 Concept A): how many sites share the most severe state that needs a look ("1
 * Unreachable", "2 Not Bound"); "All Bound" when every site reads the same; nothing when they differ but none
 * needs a look.
 */
export function groupStatus(rows: readonly OrchestrationProjectRow[]): ConnectionStatus | undefined {
  if (!rows.length) return undefined;
  const worst = rows.reduce((left, right) =>
    SEVERITY[right.connection.tone] > SEVERITY[left.connection.tone] ? right : left,
  ).connection;
  if (worst.tone === 'danger' || worst.tone === 'warning') {
    const count = rows.filter((row) => row.connection.label === worst.label).length;
    return { label: `${String(count)} ${worst.label}`, tone: worst.tone };
  }
  const first = rows[0].connection;
  return rows.every((row) => row.connection.label === first.label)
    ? { label: `All ${first.label}`, tone: first.tone }
    : undefined;
}

const joined = (...parts: (string | undefined)[]) => parts.filter(Boolean).join(' · ') || undefined;

/**
 * The tree for `projectRows` output, which lists each project with several sites as its home site's row followed
 * by its sites' rows (`siteOf` set). A site whose project row is missing stays a leaf of its own.
 */
export function projectTree(rows: readonly OrchestrationProjectRow[]): ProjectTreeNode[] {
  const homes = new Set(rows.flatMap((row) => (row.siteOf ? [row.siteOf] : [])));
  const present = new Set(rows.map((row) => row.targetId));
  const nodes: ProjectTreeNode[] = [];
  for (const row of rows) {
    if (row.siteOf && present.has(row.siteOf)) continue;
    if (!homes.has(row.targetId)) {
      nodes.push({
        id: row.targetId,
        kind: 'single',
        row,
        label: row.label,
        description: joined(row.where, row.system),
        status: row.connection,
      });
      continue;
    }
    const sites = rows.filter((site) => site.siteOf === row.targetId),
      all = [row, ...sites];
    nodes.push({
      id: projectGroupId(row.targetId),
      kind: 'project',
      row,
      label: row.label,
      description: siteSummary(all),
      status: groupStatus(all),
      children: [
        {
          id: row.targetId,
          kind: 'home',
          row,
          label: row.label,
          description: joined('Home Site', row.place, row.system),
          status: row.connection,
        },
        ...sites.map((site) => ({
          id: site.targetId,
          kind: 'site' as const,
          row: site,
          label: site.label,
          description: joined('Site', site.place, site.system),
          status: site.connection,
        })),
      ],
    });
  }
  return nodes;
}

/** Every node, groups and their children, depth first. */
export function flattenProjectTree(nodes: readonly ProjectTreeNode[]): ProjectTreeNode[] {
  return nodes.flatMap((node) => [node, ...(node.children ?? [])]);
}

/** The open groups: every group is open unless this orchestration project's person collapsed it here. */
export function expandedProjectGroups(
  nodes: readonly ProjectTreeNode[],
  ownerId: string,
  collapsed: ReadonlySet<string>,
): string[] {
  return nodes
    .filter((node) => node.kind === 'project' && !collapsed.has(projectCollapseKey(ownerId, targetOfNode(node.id))))
    .map((node) => node.id);
}

/**
 * The node the tree shows selected: the group when the person chose it and its home site is the inspector's
 * target, otherwise the target's own row.
 */
export function selectedProjectNode(selectedTarget: string | undefined, selectedNode?: string): string | undefined {
  if (!selectedTarget) return undefined;
  return selectedNode && targetOfNode(selectedNode) === selectedTarget ? selectedNode : selectedTarget;
}

/** The per-browser collapse preference (a view preference; nothing reaches the server). */
export const PROJECT_COLLAPSE_STORAGE_KEY = 'hotsheet.orchestration.projects.collapsed';

/** The preference key of one project group in one orchestration project. */
export const projectCollapseKey = (ownerId: string, homeTargetId: string) => `${ownerId}\n${homeTargetId}`;

/** The stored collapsed groups; anything unreadable or malformed reads as none collapsed (every group open). */
export function readCollapsedProjectGroups(storage: Pick<Storage, 'getItem'> | undefined): ReadonlySet<string> {
  try {
    const stored: unknown = JSON.parse(storage?.getItem(PROJECT_COLLAPSE_STORAGE_KEY) ?? '[]');
    return new Set(Array.isArray(stored) ? stored.filter((value): value is string => typeof value === 'string') : []);
  } catch {
    return new Set();
  }
}

/** `collapsed` with one group set open or closed, written back to `storage` when it changed. */
export function withProjectGroupCollapsed(
  collapsed: ReadonlySet<string>,
  key: string,
  close: boolean,
  storage: Pick<Storage, 'setItem'> | undefined,
): ReadonlySet<string> {
  if (collapsed.has(key) === close) return collapsed;
  const next = new Set(collapsed);
  if (close) next.add(key);
  else next.delete(key);
  try {
    storage?.setItem(PROJECT_COLLAPSE_STORAGE_KEY, JSON.stringify([...next]));
  } catch {
    // The preference still applies for this page when storage is unavailable.
  }
  return next;
}
