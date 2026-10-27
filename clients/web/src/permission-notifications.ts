export type PermissionDecision = 'allow' | 'deny';
/** `chat` remembers the answer for the asking chat only, in memory (HS2-JNHAYB). */
export type PermissionScope = 'once' | 'chat' | 'always';
export type PermissionHistoryDecision = PermissionDecision | 'external';
export type PermissionAutomationAction = 'off' | 'allow' | 'deny';
export interface PermissionSecretSuggestion {
  start: number;
  end: number;
  label: string;
  placeholder: 'token' | 'quoted';
}

export interface WirePermissionRequest {
  id: number;
  project?: string;
  connection: string;
  tool: string;
  action: string;
  agent?: string;
  always_allow_supported?: boolean;
  suggestions?: PermissionSecretSuggestion[];
}
export interface ToolConnection {
  id: string;
  tool: string;
  project: string;
  role: 'main' | 'worker' | 'drivespawned';
  busy: boolean;
}
export interface PermissionProject {
  id: string;
  name: string;
  root: string;
  stores?: string[];
  apiPath: string;
}
export interface PermissionItem extends WirePermissionRequest {
  key: string;
  projectId: string;
  projectName: string;
  projectRoot?: string;
  agent: string;
  role: string;
  receivedAt: number;
  ignored: boolean;
}
export interface PermissionHistoryItem extends PermissionItem {
  decision: PermissionHistoryDecision;
  scope?: PermissionScope;
  resolvedAt: number;
  automatic?: boolean;
}
/** A remembered answer as the authenticated `GET /permissions/rules` route lists it. */
export interface PermissionRule {
  project: string;
  connection?: string;
  tool: string;
  action: string;
  /** Decrypted command/action, confined to the authenticated response and local view. */
  exact_action?: string;
  match_type?: 'exact' | 'template' | 'regex';
  pattern?: PermissionPattern;
  /** Legacy label retained for older clients, never used as the rule identity. */
  description?: string;
  decision: PermissionDecision;
  scope: 'chat' | 'session' | 'always';
}

export interface PermissionPattern {
  version: 1;
  kind: 'template' | 'regex';
  expression: string;
}

/** The remembered rules that belong to a checkout, including visible legacy store rules. */
export function projectPermissionRules(
  rules: readonly PermissionRule[],
  project: Pick<PermissionProject, 'id' | 'root' | 'stores'>,
): PermissionRule[] {
  return rules.filter(
    (rule) =>
      rule.project === `checkout:${project.id}` ||
      rule.project === project.id ||
      rule.project === project.root ||
      project.stores?.includes(rule.project),
  );
}

/** The identity a client uses to name one remembered rule (row keys and revoke targets). */
export function permissionRuleKey(rule: Pick<PermissionRule, 'project' | 'connection' | 'tool' | 'action'>): string {
  return JSON.stringify([rule.project, rule.connection ?? '', rule.tool, rule.action]);
}

export interface PermissionAutomation {
  action: PermissionAutomationAction;
  delayMs: number;
}

/**
 * Every automatic-decision delay. `0` is the immediate Auto-allow option (HS2-EBGCGW): the request is
 * allowed and recorded without ever presenting the popup. It is deliberately not offered for
 * Auto-deny; use {@link permissionDelaysFor} for the delays a given action may choose.
 */
export const PERMISSION_DELAYS = [0, 5_000, 15_000, 60_000, 120_000, 300_000, 900_000, 3_600_000] as const;
export const DEFAULT_PERMISSION_AUTOMATION: PermissionAutomation = { action: 'off', delayMs: 60_000 };
const IMMEDIATE_DENY_FALLBACK_DELAY = 15_000;

/** The delays the settings form offers for `action`; immediate (0 s) is Auto-allow only. */
export function permissionDelaysFor(action: PermissionAutomationAction): readonly number[] {
  return action === 'deny' ? PERMISSION_DELAYS.filter((value) => value > 0) : PERMISSION_DELAYS;
}

/** Human label for an automatic-decision delay, as shown in the "After visible for" select. */
export function formatPermissionDelay(value: number): string {
  if (value < 60_000) {
    const seconds = value / 1000;
    return `${seconds} second${seconds === 1 ? '' : 's'}`;
  }
  return `${value / 60_000} minute${value === 60_000 ? '' : 's'}`;
}

/** Whether `automation` allows a request immediately, so the popup must never be presented. */
export function allowsImmediately(automation: PermissionAutomation): boolean {
  return automation.action === 'allow' && automation.delayMs === 0;
}

export function permissionBelongsToProject(
  request: WirePermissionRequest,
  connections: ToolConnection[],
  projects: PermissionProject[],
  currentId: string,
): boolean {
  const identity = request.project || connections.find((item) => item.id === request.connection)?.project;
  if (!identity) return true;
  const owner = projects.find(
    (item) => item.id === identity || item.root === identity || item.stores?.includes(identity),
  );
  return !owner || owner.id === currentId;
}

export function parsePermissionAutomation(raw: unknown): PermissionAutomation {
  if (!raw || typeof raw !== 'object') return DEFAULT_PERMISSION_AUTOMATION;
  const value = raw as Partial<PermissionAutomation>;
  const action = value.action === 'allow' || value.action === 'deny' ? value.action : 'off';
  const delayMs = PERMISSION_DELAYS.includes(value.delayMs as (typeof PERMISSION_DELAYS)[number])
    ? value.delayMs!
    : 60_000;
  // Immediate (0 s) is Auto-allow only; a deny (for example after switching from Auto-allow 0 s)
  // keeps the established 15-second fallback rather than denying without showing the request (HS2-0XF17D).
  return { action, delayMs: action === 'deny' && delayMs === 0 ? IMMEDIATE_DENY_FALLBACK_DELAY : delayMs };
}

export function parsePermissionHistory(raw: unknown, now = Date.now()): PermissionHistoryItem[] {
  if (!Array.isArray(raw)) return [];
  const cutoff = now - 7 * 24 * 60 * 60 * 1000;
  return raw
    .filter(
      (item): item is PermissionHistoryItem =>
        Boolean(item) &&
        typeof item === 'object' &&
        typeof (item as PermissionHistoryItem).key === 'string' &&
        typeof (item as PermissionHistoryItem).resolvedAt === 'number' &&
        (item as PermissionHistoryItem).resolvedAt >= cutoff,
    )
    .slice(-200);
}

export function formatPermissionCountdown(milliseconds: number): string {
  const total = Math.ceil(Math.max(0, milliseconds) / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export function parsePermissionResolution(
  message?: string,
): { decision: PermissionDecision; scope: PermissionScope } | undefined {
  const [decision, scope] = message?.split(':') ?? [];
  if (decision !== 'allow' && decision !== 'deny') return undefined;
  return { decision, scope: scope === 'always' ? 'always' : scope === 'chat' ? 'chat' : 'once' };
}

export class PermissionInbox {
  private pendingItems = new Map<string, PermissionItem>();
  private historyItems: PermissionHistoryItem[];
  constructor(history: unknown = []) {
    this.historyItems = parsePermissionHistory(history);
  }
  pending() {
    return [...this.pendingItems.values()].sort((a, b) => a.receivedAt - b.receivedAt);
  }
  history(now = Date.now()) {
    const cutoff = now - 7 * 24 * 60 * 60 * 1000;
    return this.historyItems
      .map((item, index) => ({ item, index }))
      .filter(({ item }) => item.resolvedAt >= cutoff)
      .sort((a, b) => b.item.resolvedAt - a.item.resolvedAt || b.index - a.index)
      .map(({ item }) => item);
  }
  visible() {
    return this.pending().find((item) => !item.ignored);
  }
  /**
   * Pending requests identical to `key`'s — same project, chat, tool, and action — oldest first,
   * including `key` itself (HS2-JNHAYB). The popup shows them as one group and one answer covers it.
   */
  identical(key: string) {
    const item = this.pendingItems.get(key);
    if (!item) return [];
    return this.pending().filter(
      (other) =>
        other.projectId === item.projectId &&
        other.connection === item.connection &&
        other.tool === item.tool &&
        other.action === item.action,
    );
  }
  reconcile(
    project: PermissionProject,
    requests: WirePermissionRequest[],
    connections: ToolConnection[],
    now = Date.now(),
  ) {
    let changed = false;
    const currentKeys = new Set(requests.map((request) => `${project.id}:${request.id}`));
    for (const item of this.pending())
      if (item.projectId === project.id && !currentKeys.has(item.key)) {
        this.pendingItems.delete(item.key);
        this.record({ ...item, decision: 'external', resolvedAt: now });
        changed = true;
      }
    for (const request of requests) {
      const key = `${project.id}:${request.id}`,
        existing = this.pendingItems.get(key),
        connection = connections.find((item) => item.id === request.connection);
      if (!existing) {
        this.pendingItems.set(key, {
          ...request,
          key,
          projectId: project.id,
          projectName: project.name,
          projectRoot: project.root,
          agent: displayAgent(connection?.tool || request.agent),
          role: connection?.role === 'worker' ? 'worker' : connection?.role === 'main' ? 'main worker' : '',
          receivedAt: now,
          ignored: false,
        });
        changed = true;
        continue;
      }
      const next = {
        ...existing,
        ...request,
        projectName: project.name,
        projectRoot: project.root,
        agent: displayAgent(connection?.tool || request.agent || existing.agent),
        role: connection?.role === 'worker' ? 'worker' : connection?.role === 'main' ? 'main worker' : '',
      };
      if (
        existing.connection !== next.connection ||
        existing.tool !== next.tool ||
        existing.action !== next.action ||
        existing.always_allow_supported !== next.always_allow_supported ||
        existing.projectName !== next.projectName ||
        existing.projectRoot !== next.projectRoot ||
        existing.agent !== next.agent ||
        existing.role !== next.role
      ) {
        this.pendingItems.set(key, next);
        changed = true;
      }
    }
    return changed;
  }
  discardProject(projectId: string) {
    let changed = false;
    for (const item of this.pendingItems.values())
      if (item.projectId === projectId) {
        this.pendingItems.delete(item.key);
        changed = true;
      }
    return changed;
  }
  ignore(key: string) {
    const item = this.pendingItems.get(key);
    if (item) this.pendingItems.set(key, { ...item, ignored: true });
  }
  present(key: string) {
    const item = this.pendingItems.get(key);
    if (item) this.pendingItems.set(key, { ...item, ignored: false });
  }
  resolve(key: string, decision: PermissionDecision, scope: PermissionScope, automatic = false, now = Date.now()) {
    const item = this.pendingItems.get(key);
    if (!item) {
      // A collection refresh can observe the empty queue before the resolution event.
      // Replace only that provisional external history with the server's known answer.
      const external = this.historyItems.find((value) => value.key === key && value.decision === 'external');
      if (!external) return false;
      this.record({ ...external, decision, scope, resolvedAt: now, automatic });
      return true;
    }
    this.pendingItems.delete(key);
    this.record({ ...item, decision, scope, resolvedAt: now, automatic });
    return true;
  }
  removeExternal(key: string, now = Date.now()) {
    const item = this.pendingItems.get(key);
    if (!item) return false;
    this.pendingItems.delete(key);
    this.record({ ...item, decision: 'external', resolvedAt: now });
    return true;
  }
  restore(item: PermissionItem) {
    this.historyItems = this.historyItems.filter((value) => value.key !== item.key);
    this.pendingItems.set(item.key, { ...item, ignored: false });
  }
  private record(item: PermissionHistoryItem) {
    this.historyItems = [...this.historyItems.filter((value) => value.key !== item.key), item].slice(-200);
  }
}

function displayAgent(agent?: string) {
  if (!agent) return 'AI tool';
  if (agent.toLowerCase() === 'codex') return 'Codex';
  if (agent.toLowerCase() === 'claude') return 'Claude';
  return agent;
}

export class VisiblePermissionTimer {
  private remaining = new Map<string, number>();
  private active?: { key: string; at: number };
  private cancelled = new Set<string>();
  show(key: string, delayMs: number, now = Date.now()) {
    if (!this.remaining.has(key)) this.remaining.set(key, delayMs);
    this.active = { key, at: now };
  }
  hide(now = Date.now()) {
    this.consume(now);
    this.active = undefined;
  }
  cancel(key: string, now = Date.now()) {
    this.consume(now);
    this.cancelled.add(key);
    if (this.active?.key === key) this.active = undefined;
  }
  isCancelled(key: string) {
    return this.cancelled.has(key);
  }
  remove(key: string) {
    this.remaining.delete(key);
    this.cancelled.delete(key);
    if (this.active?.key === key) this.active = undefined;
  }
  tick(key: string, delayMs: number, now = Date.now()) {
    if (this.cancelled.has(key)) return undefined;
    if (this.active?.key !== key) {
      this.hide(now);
      this.show(key, delayMs, now);
    }
    this.consume(now);
    if (this.active) this.active.at = now;
    return this.remaining.get(key) ?? delayMs;
  }
  private consume(now: number) {
    if (!this.active) return;
    const value = this.remaining.get(this.active.key) ?? 0;
    this.remaining.set(this.active.key, Math.max(0, value - (now - this.active.at)));
    this.active.at = now;
  }
}
