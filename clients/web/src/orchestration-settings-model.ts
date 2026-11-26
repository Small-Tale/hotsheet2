/**
 * Pure helpers behind Project Settings → General → Project Type and Settings → Orchestration
 * (HS2-W27N1D, docs/26 §26.25). The controller and components call these; nothing here reads the
 * network or the DOM.
 */
import type {
  HostedOrchestrationProjectChoice,
  OrchestrationBinding,
  OrchestrationDestination,
  OrchestrationManifest,
  OrchestrationPolicyBody,
  OrchestrationPolicyView,
  OrchestrationResolution,
  OrchestrationRoleSession,
  OrchestrationSession,
  OrchestrationSitesView,
  OrchestrationTargetPolicy,
} from './api';
import { systemLabel } from './orchestration-capabilities';
import { parseRemoteProjectId, remoteProjectId } from './remote-projects/identity';

/** What an add asks the server to bind: a checkout here or on a reviewed remote registration. */
export type AddDestination =
  { kind: 'local'; checkout_id: string } | { kind: 'remote'; registration_id: string; checkout_id: string };

/** The machine a project lives on, as the person reads it. */
export const LOCAL_MACHINE_LABEL = 'This Computer';
export const UNKNOWN_REMOTE_LABEL = 'Remote Server';

/** An open tab's project as the add needs it: local tab ids are checkout ids, remote ones encode both. */
export function destinationOf(projectId: string): AddDestination {
  const remote = parseRemoteProjectId(projectId);
  return remote
    ? { kind: 'remote', registration_id: remote.bindingId, checkout_id: remote.checkoutId }
    : { kind: 'local', checkout_id: projectId };
}

/**
 * The open tab that shows a bound project, so its row can show that tab or its sessions. Bindings are
 * the serving server's: when that is another server (the settings tab `servingProjectId` is a remote
 * tab), a checkout local to it opens here as a remote tab of the same registration, and a destination
 * remote to it names a registration of that server, not of this client, so no tab here is known
 * (HS2-7SHGQH).
 */
export function tabProjectIdOf(destination: OrchestrationDestination, servingProjectId?: string): string | undefined {
  const serving = servingProjectId ? parseRemoteProjectId(servingProjectId) : undefined;
  if (serving)
    return destination.kind === 'local' ? remoteProjectId(serving.bindingId, destination.checkout_id) : undefined;
  return destination.kind === 'local'
    ? destination.checkout_id
    : remoteProjectId(destination.registration_id, destination.checkout_id);
}

const placeOf = (destination: AddDestination | OrchestrationDestination) =>
  destination.kind === 'local'
    ? `local:${destination.checkout_id}`
    : `remote:${destination.registration_id}:${destination.checkout_id}`;

export function machineOf(
  destination: AddDestination | OrchestrationDestination,
  remoteLabels: Readonly<Record<string, string>>,
): string {
  return destination.kind === 'local'
    ? LOCAL_MACHINE_LABEL
    : (remoteLabels[destination.registration_id] ?? UNKNOWN_REMOTE_LABEL);
}

export type StatusTone = 'success' | 'warning' | 'danger' | 'neutral';
export interface ConnectionStatus {
  label: string;
  tone: StatusTone;
}

const FAILURE_LABELS: Readonly<Record<string, string>> = {
  unreachable: 'Unreachable',
  unauthorized: 'Not Authorized',
  source_missing: 'Project Missing',
  identity_changed: 'Identity Changed',
  revoked: 'Server Removed',
  missing_credentials: 'Sign-In Needed',
  trust_failed: 'Not Trusted',
  review_required: 'Review Needed',
  disabled_source: 'Source Disabled',
  target_removed: 'Removed',
};

/** The connection state words and tone for a row or the detail. A fresh resolution wins over the cache. */
export function connectionStatus(
  binding: OrchestrationBinding | undefined,
  resolution?: OrchestrationResolution,
): ConnectionStatus {
  if (resolution) {
    if (resolution.usable) return { label: 'Connected', tone: 'success' };
    if (resolution.state === 'unbound') return { label: 'Not Bound', tone: 'warning' };
    if (resolution.state === 'connecting') return { label: 'Connecting', tone: 'neutral' };
    return {
      label: FAILURE_LABELS[resolution.reason ?? resolution.state] ?? 'Unavailable',
      tone: 'danger',
    };
  }
  if (!binding) return { label: 'Checking', tone: 'neutral' };
  if (!binding.destination) return { label: 'Not Bound', tone: 'warning' };
  if (binding.last_failure) return { label: FAILURE_LABELS[binding.last_failure] ?? 'Unavailable', tone: 'danger' };
  return { label: binding.confirmed_at_ms ? 'Bound' : 'Not Confirmed', tone: 'neutral' };
}

export interface OrchestrationProjectRow {
  targetId: string;
  label: string;
  location: 'local' | 'remote' | 'unbound';
  /**
   * "Local on This Computer", "Remote on Build host", or "Not bound on this computer"; for a project served by
   * another server "On build.example" or "Not bound on build.example" (HS2-DXAB4K).
   */
  where: string;
  /** HS2-D4G81Q: the machine part of `where` alone: "Remote on Build host", or empty when unknown. */
  place: string;
  connection: ConnectionStatus;
  /** The open tab that shows this project, when it is bound. */
  tabProjectId?: string;
  /** HS2-SHH7WW: the project target this row is another site of. */
  siteOf?: string;
  /** HS2-D4G81Q: the OS of a site's machine (`macOS 15.1, arm64`), once the sites view read it. */
  system?: string;
}

/**
 * The orchestrated projects: active declarations with this machine's binding, by label. HS2-SHH7WW: a
 * project with several sites lists its own row first and then each other site, labelled with its project.
 * HS2-DXAB4K: `servedBy` names the server an orchestration project open from another server is served by
 * (the tab's remote host). Its bindings are that server's, so a checkout local to it is "On <server>",
 * never "Local on This Computer".
 */
export function projectRows(
  manifest: OrchestrationManifest | undefined,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
  remoteLabels: Readonly<Record<string, string>>,
  sites?: OrchestrationSitesView,
  servedBy?: string,
  servingProjectId?: string,
): OrchestrationProjectRow[] {
  // HS2-D4G81Q: a site's OS shows on the rows of a project with several sites.
  const capabilities = new Map(
    (sites?.projects ?? [])
      .filter((project) => project.sites.length > 1)
      .flatMap((project) => project.sites)
      .flatMap((site) => (site.capabilities ? [[site.target_id, site.capabilities] as const] : [])),
  );
  const active = (manifest?.targets ?? []).filter((target) => target.state === 'active'),
    byLabel = (left: { label: string }, right: { label: string }) => left.label.localeCompare(right.label),
    projectOf = (target: (typeof active)[number]) =>
      target.site_of && active.some((other) => other.target_id === target.site_of && !other.site_of)
        ? target.site_of
        : undefined;
  const row = (target: (typeof active)[number], prefix: string): OrchestrationProjectRow => {
    const binding = bindings[target.target_id],
      machine = capabilities.get(target.target_id),
      destination = binding?.destination ?? undefined,
      place = destination
        ? servedBy !== undefined && destination.kind === 'local'
          ? `On ${servedBy}`
          : `${destination.kind === 'local' ? 'Local' : 'Remote'} on ${machineOf(destination, remoteLabels)}`
        : binding
          ? `Not bound on ${servedBy ?? 'this computer'}`
          : '';
    return {
      targetId: target.target_id,
      label: target.label,
      // HS2-DXAB4K: a checkout of the serving server is remote from here, as its + Add Project choice shows.
      location: destination
        ? servedBy !== undefined
          ? 'remote'
          : destination.kind
        : binding
          ? 'unbound'
          : servedBy !== undefined
            ? 'remote'
            : 'local',
      where: [prefix, place].filter(Boolean).join(' · '),
      place,
      connection: connectionStatus(binding),
      tabProjectId: destination ? tabProjectIdOf(destination, servingProjectId) : undefined,
      siteOf: projectOf(target),
      system: machine ? systemLabel(machine) : undefined,
    };
  };
  return active
    .filter((target) => !projectOf(target))
    .sort(byLabel)
    .flatMap((project) => {
      const sites = active.filter((target) => projectOf(target) === project.target_id).sort(byLabel),
        siteRows = sites.map((site) => row(site, `Site of ${project.label}`));
      if (!sites.length) return [row(project, '')];
      // HS2-NGP2AK: a project with several sites names where every one of them is, its own first.
      const own = row(project, ''),
        places = [...new Set([own.place, ...siteRows.map((site) => site.place)].filter(Boolean))];
      return [
        { ...own, where: [`${String(sites.length + 1)} sites`, places.join(', ')].filter(Boolean).join(' · ') },
        ...siteRows,
      ];
    });
}

const SITE_READ_FAILURES: Readonly<Record<string, string>> = {
  target_unavailable: 'Cannot be reached now',
  policy_denied: 'The policy does not allow reading it',
  permission_required: 'Waits for a person to answer a permission request',
  permission_denied: 'A person denied reading it',
};
/** HS2-D4G81Q: why a site's machine could not be read, from the sites view's error code. */
export function siteReadFailure(code: string): string {
  return SITE_READ_FAILURES[code] ?? `Could not be read (${code})`;
}

export interface AddChoice {
  /** The open tab's project id. */
  projectId: string;
  name: string;
  location: 'local' | 'remote';
  machine: string;
  destination: AddDestination;
  /** Already orchestrated: shown checked, and choosing it does nothing more. */
  added: boolean;
}

/** The + Add Project menu's Open Projects: every other open tab, with whether it is already added. */
export function addChoices(
  openProjects: readonly { id: string; name: string }[],
  ownerId: string,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
  manifest: OrchestrationManifest | undefined,
  remoteLabels: Readonly<Record<string, string>>,
): AddChoice[] {
  const active = new Set(
      (manifest?.targets ?? []).filter((target) => target.state === 'active').map((target) => target.target_id),
    ),
    bound = new Set(
      Object.entries(bindings)
        .filter(([target, binding]) => active.has(target) && binding?.destination)
        .map(([, binding]) => placeOf(binding!.destination!)),
    );
  return openProjects
    .filter((project) => project.id !== ownerId)
    .map((project) => {
      const destination = destinationOf(project.id);
      return {
        projectId: project.id,
        name: project.name,
        location: destination.kind,
        machine: machineOf(destination, remoteLabels),
        destination,
        added: bound.has(placeOf(destination)),
      };
    });
}

/**
 * HS2-E9VVQF: the ticket store each added project reaches, as this machine's bindings of its sites
 * name it (`checkout_authority`). A project with no bound site has no known store.
 */
export function projectStores(
  manifest: OrchestrationManifest | undefined,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
): Map<string, string> {
  const active = (manifest?.targets ?? []).filter((target) => target.state === 'active'),
    projects = new Set(active.filter((target) => !target.site_of).map((target) => target.target_id)),
    stores = new Map<string, string>();
  // The project's own binding first, then its sites in declaration order.
  for (const target of [...active].sort(
    (left, right) => Number(Boolean(left.site_of)) - Number(Boolean(right.site_of)),
  )) {
    const project = target.site_of && projects.has(target.site_of) ? target.site_of : target.target_id,
      authority = bindings[target.target_id]?.destination?.checkout_authority;
    if (authority && !stores.has(project)) stores.set(project, authority);
  }
  return stores;
}

/** HS2-E9VVQF: one open project a project row's Add a Site offers. */
export interface SiteChoice {
  projectId: string;
  name: string;
  location: 'local' | 'remote';
  machine: string;
  /** Reaches the project's ticket store; `false` for another store, `undefined` when not known. */
  sameStore: boolean | undefined;
}

/**
 * HS2-E9VVQF: Add a Site's open projects for one added project: by default only those that reach its
 * ticket store; with Show All Projects every open project not added yet, so the server's refusal can
 * explain an edge case. `stores` maps an open tab to the store it reaches, `null` when it could not
 * be inspected.
 */
export function siteChoices(
  choices: readonly AddChoice[],
  stores: Readonly<Record<string, string | null | undefined>>,
  projectStore: string | undefined,
  showAll: boolean,
): SiteChoice[] {
  return choices
    .filter((choice) => !choice.added)
    .map((choice) => {
      const store = stores[choice.projectId];
      return {
        projectId: choice.projectId,
        name: choice.name,
        location: choice.location,
        machine: choice.machine,
        sameStore: store && projectStore ? store === projectStore : undefined,
      };
    })
    .filter((choice) => showAll || choice.sameStore === true);
}

/**
 * HS2-89YNGN: a project open from another server adds that server's open checkouts. As Add a Site and the duplicate
 * check read them: each checkout as an add choice on that server, and the store each reaches (`null` when unread).
 */
export function hostedSiteSources(
  hosted: readonly HostedOrchestrationProjectChoice[],
  server: string,
): { choices: AddChoice[]; stores: Record<string, string | null> } {
  return {
    choices: hosted.map((choice) => ({
      projectId: choice.checkout_id,
      name: choice.name,
      location: 'remote' as const,
      machine: server,
      destination: { kind: 'local' as const, checkout_id: choice.checkout_id },
      added: choice.added,
    })),
    stores: Object.fromEntries(hosted.map((choice) => [choice.checkout_id, choice.checkout_authority ?? null])),
  };
}

/** HS2-E9VVQF: every project row's Add a Site choices, by the project's target id. */
export function siteChoicesByProject(
  rows: readonly OrchestrationProjectRow[],
  choices: readonly AddChoice[],
  stores: Readonly<Record<string, string | null | undefined>>,
  manifest: OrchestrationManifest | undefined,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
  showAll: boolean,
): Record<string, SiteChoice[]> {
  const known = projectStores(manifest, bindings);
  return Object.fromEntries(
    rows
      .filter((row) => !row.siteOf)
      .map((row) => [row.targetId, siteChoices(choices, stores, known.get(row.targetId), showAll)]),
  );
}

/**
 * HS2-E9VVQF: + Add Project's duplicate check. The added project that reaches the same ticket store as
 * the chosen open project, if any, so the person can say whether it is another site of that project.
 */
export function duplicateProject(
  store: string | null | undefined,
  manifest: OrchestrationManifest | undefined,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
): { targetId: string; label: string } | undefined {
  if (!store) return undefined;
  const stores = projectStores(manifest, bindings);
  return (manifest?.targets ?? [])
    .filter((target) => target.state === 'active' && stores.get(target.target_id) === store)
    .sort((left, right) => left.label.localeCompare(right.label))
    .map((target) => ({ targetId: target.target_id, label: target.label }))[0];
}

/** A canonical ULID for an add the person may retry (docs/26 §26.4: retain the id before setup). */
export function newUlid(now = Date.now(), random: (bytes: Uint8Array) => Uint8Array = defaultRandom): string {
  const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let time = '';
  let rest = Math.max(0, Math.floor(now));
  for (let index = 0; index < 10; index += 1) {
    time = alphabet[rest % 32] + time;
    rest = Math.floor(rest / 32);
  }
  const bytes = random(new Uint8Array(16));
  let tail = '';
  for (let index = 0; index < 16; index += 1) tail += alphabet[bytes[index] % 32];
  return time + tail;
}

function defaultRandom(bytes: Uint8Array): Uint8Array {
  return globalThis.crypto.getRandomValues(bytes);
}

/* ---- Sessions ---------------------------------------------------------------------------- */

const ROLE_LABELS = { worker: 'Worker', dispatcher: 'Dispatcher', integrator: 'Integrator' } as const;
const STATE_LABELS = {
  requested: 'Starting',
  running: 'Running',
  stopped: 'Stopped',
  failed: 'Failed',
  lost: 'Lost',
} as const;

export function sessionTitle(session: OrchestrationSession, toolLabel: (tool: string) => string): string {
  return session.label ?? `${toolLabel(session.tool)} ${ROLE_LABELS[session.role]}`;
}

/**
 * A session row's second line: its role first when it is a dispatcher or integrator (HS2-TK9ADC), then
 * its tool, model and start.
 */
export function sessionDescription(
  session: OrchestrationSession,
  toolLabel: (tool: string) => string,
  since: string,
): string {
  const role = session.role === 'worker' ? '' : `${ROLE_LABELS[session.role]} · `;
  return `${role}${toolLabel(session.tool)}${session.model ? ` · ${session.model}` : ''} · since ${since}`;
}

export function sessionStatus(session: OrchestrationSession): ConnectionStatus {
  if (session.owner === 'human') return { label: 'Taken Over', tone: 'neutral' };
  if (session.paused) return { label: 'Paused', tone: 'warning' };
  return {
    label: STATE_LABELS[session.state],
    tone: session.state === 'running' ? 'success' : session.state === 'requested' ? 'neutral' : 'danger',
  };
}

/** Megabytes as the person reads them. */
export function formatMegabytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return 'Unknown';
  return value >= 1024 ? `${(value / 1024).toFixed(1)} GB` : `${String(value)} MB`;
}

/* ---- Policy, limits only (HS2-XHQ1EH) ---------------------------------------------------- */

/** The defaults the server applies to an absent member (docs/26 §26.8). */
export const POLICY_DEFAULTS = {
  machine: { max_sessions: 2, min_free_ram_mb: 2048, max_cpu_percent: 90, min_free_disk_mb: 5120 },
  /** HS2-N3HWP9: Auto weight, an equal share. */
  tool: { weight: 50 },
  wind_down: { idle_minutes: 15, keep_warm: 0 },
} as const;

/** One tool account (HS2-7WF6JD). An empty account name drops the row; an empty budget sets none. */
export interface PolicyAccountDraft {
  account: string;
  max_sessions: string;
  /** Dollars per day, as typed. */
  budget: string;
}

export interface PolicyToolDraft {
  tool: string;
  /** HS2-N3HWP9: empty is Auto (no cap of its own), 0 turns the tool off. */
  max_sessions: string;
  /** HS2-N3HWP9: empty is Auto (an equal share), 0 turns the tool off. */
  weight: string;
  accounts: PolicyAccountDraft[];
}

export interface PolicyDraft {
  /** HS2-R2N33M: undefined is Auto, on once a plan exists; a person's switch makes it explicit. */
  automatic: boolean | undefined;
  machine: { max_sessions: string; min_free_ram_mb: string; max_cpu_percent: string; min_free_disk_mb: string };
  tools: PolicyToolDraft[];
  idle_minutes: string;
  keep_warm: string;
  build_ahead: boolean;
  /** HS2-NCMATS: `speculation.redo_reopened`. */
  redo_reopened: boolean;
}

export const MACHINE_FIELDS = ['max_sessions', 'min_free_ram_mb', 'max_cpu_percent', 'min_free_disk_mb'] as const;
export type MachineField = (typeof MACHINE_FIELDS)[number];

/** A form draft from the stored policy. Every AI tool the app knows gets a row, configured or not. */
export function policyDraftFrom(view: OrchestrationPolicyView | undefined, knownTools: readonly string[]): PolicyDraft {
  const body: OrchestrationPolicyBody = view?.policy ?? {},
    machine = { ...POLICY_DEFAULTS.machine, ...body.machine },
    configured = body.tools ?? [],
    names = [
      ...configured.map((tool) => tool.tool),
      ...knownTools.filter((tool) => !configured.some((item) => item.tool === tool)),
    ];
  return {
    automatic: body.placement?.automatic,
    machine: {
      max_sessions: String(machine.max_sessions),
      min_free_ram_mb: String(machine.min_free_ram_mb),
      max_cpu_percent: String(machine.max_cpu_percent),
      min_free_disk_mb: String(machine.min_free_disk_mb),
    },
    tools: names.map((name) => {
      const tool = configured.find((item) => item.tool === name);
      return {
        tool: name,
        max_sessions: autoOrNumber(tool?.max_sessions),
        weight: autoOrNumber(tool?.weight),
        accounts: accountsOf(tool?.accounts).map((account) => ({
          account: account.account,
          max_sessions: String(account.max_sessions),
          budget: account.budget_cents_per_day === undefined ? '' : formatCents(account.budget_cents_per_day),
        })),
      };
    }),
    idle_minutes: String(body.wind_down?.idle_minutes ?? POLICY_DEFAULTS.wind_down.idle_minutes),
    keep_warm: String(body.wind_down?.keep_warm ?? POLICY_DEFAULTS.wind_down.keep_warm),
    build_ahead: body.speculation?.allowed ?? false,
    redo_reopened: body.speculation?.redo_reopened ?? false,
  };
}

/** HS2-N3HWP9: whether a stored tool entry turns the tool off (Sessions or Weight 0). */
function toolOff(tool: { max_sessions?: unknown; weight?: unknown } | undefined): boolean {
  return tool?.max_sessions === 0 || tool?.weight === 0;
}

/**
 * HS2-N3HWP9: the tools sessions may use: the policy's listed tools and every AI tool the app knows
 * (an unlisted tool is Auto), less those the policy turns off.
 */
export function usablePolicyTools(
  body: OrchestrationPolicyBody | null | undefined,
  knownTools: readonly string[],
): string[] {
  const listed = body?.tools ?? [];
  return [...new Set([...listed.map((tool) => tool.tool), ...knownTools])].filter(
    (name) => !toolOff(listed.find((tool) => tool.tool === name)),
  );
}

/** One form control's change. Field names are the controls' `name` attributes. */
export function changePolicyDraft(draft: PolicyDraft, field: string, value: string | boolean): PolicyDraft {
  if (field === 'automatic' && typeof value === 'boolean') return { ...draft, automatic: value };
  if (field === 'build_ahead' && typeof value === 'boolean') return { ...draft, build_ahead: value };
  if (field === 'redo_reopened' && typeof value === 'boolean') return { ...draft, redo_reopened: value };
  if (typeof value !== 'string') return draft;
  if (field === 'idle_minutes' || field === 'keep_warm') return { ...draft, [field]: value };
  const machineField = /^machine\.(.+)$/u.exec(field)?.[1];
  if (machineField && (MACHINE_FIELDS as readonly string[]).includes(machineField))
    return { ...draft, machine: { ...draft.machine, [machineField]: value } };
  const tool = /^tool\.(.+)\.(max_sessions|weight)$/u.exec(field);
  if (tool)
    return {
      ...draft,
      tools: draft.tools.map((item) => (item.tool === tool[1] ? { ...item, [tool[2]]: value } : item)),
    };
  const account = /^account\.(.+)\.(\d+)\.(account|max_sessions|budget)$/u.exec(field);
  if (account)
    return {
      ...draft,
      tools: draft.tools.map((item) =>
        item.tool === account[1]
          ? {
              ...item,
              accounts: item.accounts.map((row, index) =>
                index === Number(account[2]) ? { ...row, [account[3]]: value } : row,
              ),
            }
          : item,
      ),
    };
  return draft;
}

/** + Add Account on a tool row (HS2-7WF6JD). */
export function addPolicyAccount(draft: PolicyDraft, tool: string): PolicyDraft {
  return {
    ...draft,
    tools: draft.tools.map((item) =>
      item.tool === tool
        ? { ...item, accounts: [...item.accounts, { account: '', max_sessions: '1', budget: '' }] }
        : item,
    ),
  };
}

/** Remove one account row of a tool. */
export function removePolicyAccount(draft: PolicyDraft, tool: string, index: number): PolicyDraft {
  return {
    ...draft,
    tools: draft.tools.map((item) =>
      item.tool === tool ? { ...item, accounts: item.accounts.filter((_row, at) => at !== index) } : item,
    ),
  };
}

interface StoredAccount {
  account: string;
  max_sessions: number;
  budget_cents_per_day?: number;
}

/** The stored accounts of a tool, skipping anything that is not an account entry. */
function accountsOf(value: unknown[] | undefined): StoredAccount[] {
  return (value ?? []).filter(
    (item): item is StoredAccount =>
      typeof item === 'object' &&
      item !== null &&
      typeof (item as StoredAccount).account === 'string' &&
      typeof (item as StoredAccount).max_sessions === 'number',
  );
}

/** Whole cents as dollars, exactly: 250 → "2.50", 300 → "3". */
export function formatCents(cents: number): string {
  const whole = Math.floor(cents / 100),
    rest = cents % 100;
  return rest === 0 ? String(whole) : `${String(whole)}.${String(rest).padStart(2, '0')}`;
}

/** Dollars as typed to whole cents, exactly; more than two decimals is refused, never rounded. */
export function parseDollars(value: string, label: string): number | string {
  const match = /^(\d+)(?:\.(\d{1,2}))?$/u.exec(value.trim());
  if (!match) return `${label} must be an amount in dollars, such as 12.50.`;
  const [, dollars, cents] = match as unknown as [string, string, string | undefined];
  return Number(dollars) * 100 + Number((cents ?? '').padEnd(2, '0'));
}

const LABELS: Readonly<Record<string, string>> = {
  max_sessions: 'Sessions per Machine',
  min_free_ram_mb: 'Free Memory Floor',
  max_cpu_percent: 'CPU Ceiling',
  min_free_disk_mb: 'Free Disk Floor',
  idle_minutes: 'Wind Down After Idle',
  keep_warm: 'Idle Sessions Kept per Project',
};

/** HS2-N3HWP9: a stored tool limit as typed; Auto (absent, null or `"auto"`) is an empty field. */
function autoOrNumber(value: unknown): string {
  return typeof value === 'number' ? String(value) : '';
}

function whole(value: string, label: string, minimum: number, maximum?: number): number | string {
  const trimmed = value.trim();
  if (!/^\d+$/u.test(trimmed)) return `${label} must be a whole number.`;
  const number = Number(trimmed);
  if (number < minimum || (maximum !== undefined && number > maximum))
    return maximum === undefined
      ? `${label} must be at least ${String(minimum)}.`
      : `${label} must be ${String(minimum)} to ${String(maximum)}.`;
  return number;
}

/**
 * The policy to store: the stored body with only the limits replaced. Targets, accounts and every
 * other member round-trip unchanged, so this form never edits what it does not show. Retired members
 * (`targets[].actions`, `approval`) pass through untouched; the server ignores and drops them
 * (HS2-XHQ1EH).
 */
export function policyFromDraft(
  view: OrchestrationPolicyView | undefined,
  draft: PolicyDraft,
): { policy: OrchestrationPolicyBody } | { error: string } {
  const body: OrchestrationPolicyBody = structuredClone(view?.policy ?? {});
  const machine: Record<string, number> = {};
  for (const field of MACHINE_FIELDS) {
    const parsed = whole(
      draft.machine[field],
      LABELS[field],
      field === 'max_sessions' || field === 'max_cpu_percent' ? 1 : 0,
      field === 'max_sessions' ? 64 : field === 'max_cpu_percent' ? 100 : undefined,
    );
    if (typeof parsed === 'string') return { error: parsed };
    machine[field] = parsed;
  }
  const idle = whole(draft.idle_minutes, LABELS.idle_minutes, 1, 1440),
    warm = whole(draft.keep_warm, LABELS.keep_warm, 0, 64);
  if (typeof idle === 'string') return { error: idle };
  if (typeof warm === 'string') return { error: warm };
  const stored = body.tools ?? [],
    tools: NonNullable<OrchestrationPolicyBody['tools']> = [];
  for (const row of draft.tools) {
    // HS2-N3HWP9: an empty field is Auto and is stored by leaving the member out; 0 turns the tool off.
    const sessions = row.max_sessions.trim() ? whole(row.max_sessions, `${row.tool} sessions`, 0, 64) : undefined,
      weight = row.weight.trim() ? whole(row.weight, `${row.tool} weight`, 0, 100) : undefined;
    if (typeof sessions === 'string') return { error: sessions };
    if (typeof weight === 'string') return { error: weight };
    const previous = stored.find((tool) => tool.tool === row.tool);
    const accounts: StoredAccount[] = [];
    for (const entry of row.accounts) {
      const name = entry.account.trim();
      if (!name) continue;
      const label = `${row.tool} account ${name}`,
        cap = whole(entry.max_sessions, `${label} sessions`, 1, sessions ? sessions : 64);
      if (typeof cap === 'string') return { error: cap };
      if (accounts.some((item) => item.account === name)) return { error: `${label} is listed twice.` };
      const budget = entry.budget.trim() ? parseDollars(entry.budget, `${label} daily budget`) : undefined;
      if (typeof budget === 'string') return { error: budget };
      accounts.push(
        budget === undefined
          ? { account: name, max_sessions: cap }
          : { account: name, max_sessions: cap, budget_cents_per_day: budget },
      );
    }
    const next: NonNullable<OrchestrationPolicyBody['tools']>[number] = { ...previous, tool: row.tool };
    if (sessions === undefined) delete next.max_sessions;
    else next.max_sessions = sessions;
    if (weight === undefined) delete next.weight;
    else next.weight = weight;
    if (accounts.length) next.accounts = accounts;
    else delete next.accounts;
    // A tool that is Auto in everything needs no entry: an unlisted tool is Auto.
    if (Object.keys(next).length > 1) tools.push(next);
  }
  body.machine = { ...body.machine, ...machine };
  body.tools = tools;
  body.wind_down = { ...body.wind_down, idle_minutes: idle, keep_warm: warm };
  body.speculation = { ...body.speculation, allowed: draft.build_ahead, redo_reopened: draft.redo_reopened };
  // HS2-R2N33M: Auto stays unset, so it keeps following whether a plan exists.
  body.placement = { ...body.placement };
  if (draft.automatic === undefined) delete body.placement.automatic;
  else body.placement.automatic = draft.automatic;
  return { policy: body };
}

/* ---- One project's limits (HS2-7WF6JD) ------------------------------------------------- */

export type BuildAheadChoice = 'default' | 'on' | 'off';

/** A project's entry in the policy as the detail's Limits form edits it. */
export interface TargetLimitsDraft {
  targetId: string;
  /** The policy lists this project. HS2-N5BDE4: an unlisted project is Auto in everything. */
  listed: boolean;
  enabled: boolean;
  /** The tools sessions here may use, in the policy's tool order. HS2-N5BDE4: none is Auto, any usable tool. */
  tools: string[];
  /** HS2-N5BDE4: empty is Auto (the machine's sessions limit), 0 starts none here. */
  max_sessions: string;
  machineOverride: boolean;
  machine: { max_sessions: string; min_free_ram_mb: string; max_cpu_percent: string; min_free_disk_mb: string };
  buildAhead: BuildAheadChoice;
  /** HS2-YSY7AR: the models each checked tool may use here, as typed (comma-separated; empty is any). */
  models: Record<string, string>;
  /** HS2-YSY7AR: the resources every piece of work here must hold, `<namespace>/<resource>`, comma-separated. */
  resources: string;
  /** HS2-TK9ADC: the role sessions the scheduler starts here by itself. */
  roleSessions: OrchestrationRoleSession[];
  /**
   * HS2-R2N33M: the policy does not list this project's role sessions, so they are Auto: both, started with
   * placement when they help. Shown checked; choosing any makes the choice explicit.
   */
  roleSessionsAuto: boolean;
}

const ROLE_SESSIONS_HINT =
  'Checked, the orchestrator starts a dispatcher for ready work or an integrator for handed-off work when needed. Each counts toward Sessions Here.';

/**
 * HS2-Y737GM: what Sessions Here and Build Ahead mean, under the fields and in the Limits help.
 * HS2-25KT42: each says what its Auto (empty or default) value does.
 */
export const SESSIONS_HERE_HINT =
  'The most AI sessions the orchestrator runs in this project at once. Empty is Auto: up to its machine’s Sessions per Machine. 0 starts none here.';
export const BUILD_AHEAD_HINT =
  'Whether work may start before a ticket it depends on has passed CI, on that ticket’s pending result. As the Policy Says follows the policy’s Build Ahead switch.';
export const REQUIRED_RESOURCES_HINT =
  'Empty is Auto: no resource is required. Choose a resource or a pattern such as vm/*: every session here must hold one match, and claims it itself.';
export const MODELS_HINT = 'Empty allows any model. Choose a model or a family such as opus-*.';

/**
 * HS2-25KT42: how many of the Advanced limits differ from Auto, for the Advanced disclosure's
 * summary: coordination sessions, Sessions Here, Build Ahead, required resources and machine limits.
 */
export function advancedLimitsChanged(draft: TargetLimitsDraft | undefined): number {
  if (!draft) return 0;
  return [
    // HS2-R2N33M: unlisted coordination sessions are Auto; any choice, even of none, is a change.
    !draft.roleSessionsAuto,
    draft.max_sessions.trim() !== '',
    draft.buildAhead !== 'default',
    listOf(draft.resources).length > 0,
    draft.machineOverride,
  ].filter(Boolean).length;
}

/**
 * HS2-SYZB33: coordination sessions start only on a project's home site (HS2-5P112Z). On another site
 * the choice points at the home and is unavailable, except to clear roles still checked here (a stored
 * entry), which have no effect.
 */
export function roleSessionsHint(homeLabel: string | undefined, checked: readonly string[], auto = false): string {
  if (!homeLabel)
    return auto ? `${ROLE_SESSIONS_HINT} Auto: follows placement, both when they help.` : ROLE_SESSIONS_HINT;
  const where = `Coordination sessions start only on ${homeLabel}, this project's home site. Set them there.`;
  return checked.length && !auto ? `${where} The roles checked here have no effect; clear them.` : where;
}

/**
 * The project's own limits in the policy (HS2-7WF6JD): whether the orchestrator acts here, the AI
 * tools sessions here may use, the session cap, a machine override and build-ahead. Applied with
 * Apply Limits, which writes only this project's entry; the policy stays limits only.
 */

/** HS2-SYZB33: on another site the roles can only be cleared, never chosen. */
export function roleSessionsLocked(homeLabel: string | undefined, checked: readonly string[], auto = false): boolean {
  return Boolean(homeLabel) && (checked.length === 0 || auto);
}

/** HS2-TK9ADC: the role sessions in the order the form and the policy list them. */
export const ROLE_SESSIONS: readonly OrchestrationRoleSession[] = ['dispatcher', 'integrator'];

const targetOf = (body: OrchestrationPolicyBody | null | undefined, targetId: string) =>
  body?.targets?.find((entry) => entry.target_id === targetId);

/** HS2-R2N33M: a project's role sessions; unlisted is Auto, shown as both. */
function roleSessionsOf(body: OrchestrationPolicyBody | null | undefined, targetId: string) {
  const listed = body?.placement?.role_sessions?.find((item) => item.target_id === targetId);
  return {
    roleSessions: ROLE_SESSIONS.filter((role) => (listed ? listed.roles.includes(role) : true)),
    roleSessionsAuto: !listed,
  };
}

/** The Limits draft of one project, from its entry or the defaults the server applies. */
export function targetLimitsDraftFrom(view: OrchestrationPolicyView | undefined, targetId: string): TargetLimitsDraft {
  const body = view?.policy,
    entry = targetOf(body, targetId),
    defaults = { ...POLICY_DEFAULTS.machine, ...body?.machine },
    machine = { ...defaults, ...entry?.machine };
  return {
    targetId,
    listed: Boolean(entry),
    enabled: entry?.enabled ?? true,
    tools: (entry?.tools ?? []).map((scope) => scope.tool),
    max_sessions: autoOrNumber(entry?.max_sessions),
    machineOverride: Boolean(entry?.machine),
    machine: {
      max_sessions: String(machine.max_sessions),
      min_free_ram_mb: String(machine.min_free_ram_mb),
      max_cpu_percent: String(machine.max_cpu_percent),
      min_free_disk_mb: String(machine.min_free_disk_mb),
    },
    buildAhead: entry?.speculation === undefined ? 'default' : entry.speculation ? 'on' : 'off',
    models: Object.fromEntries((entry?.tools ?? []).map((scope) => [scope.tool, (scope.models ?? []).join(', ')])),
    resources: (body?.placement?.resources?.find((item) => item.target_id === targetId)?.resources ?? [])
      .map((need) => `${need.namespace}/${need.resource}`)
      .join(', '),
    ...roleSessionsOf(body, targetId),
  };
}

/** The server's bounds (docs/26 §26.8, §26.20). */
const MAX_TOOL_MODELS = 32;
const MAX_PROJECT_RESOURCES = 4;
const NAMESPACE = /^[a-z0-9][a-z0-9._-]{0,63}$/u;
/** HS2-NPAZQW: a required resource may be a pattern: `*` in the resource part matches any run. */
const RESOURCE = /^[A-Za-z0-9*][A-Za-z0-9._*-]{0,127}$/u;

/** A comma-separated list as typed: trimmed, empty entries dropped. */
export const listOf = (text: string) =>
  text
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);

/** The models a tool may use here, or why the list would be refused. Empty means any model. */
export function parseModels(text: string, label: string): string[] | string {
  const models = listOf(text);
  if (models.length > MAX_TOOL_MODELS) return `${label} lists more than ${String(MAX_TOOL_MODELS)} models.`;
  for (const [index, model] of models.entries()) {
    if (/\s/u.test(model)) return `${label}: separate models with commas; "${model}" has a space.`;
    if (new TextEncoder().encode(model).length > 128) return `${label}: a model name is at most 128 bytes.`;
    if (models.indexOf(model) !== index) return `${label} lists ${model} twice.`;
  }
  return models;
}

/** Required resources as `<namespace>/<resource>` in the claim service's names, at most four, each once. */
export function parseResources(text: string): { namespace: string; resource: string }[] | string {
  const names = listOf(text);
  if (names.length > MAX_PROJECT_RESOURCES)
    return `Required Resources lists at most ${String(MAX_PROJECT_RESOURCES)} resources.`;
  const needs: { namespace: string; resource: string }[] = [];
  for (const name of names) {
    const parts = name.split('/'),
      [namespace = '', resource = ''] = parts;
    if (parts.length !== 2 || !NAMESPACE.test(namespace) || namespace.includes('..') || !RESOURCE.test(resource))
      return `"${name}" is not <namespace>/<resource>, such as vm/macos-vm or vm/*: a lowercase namespace and a resource name or pattern.`;
    if (needs.some((need) => need.namespace === namespace && need.resource === resource))
      return `Required Resources lists ${name} twice.`;
    needs.push({ namespace, resource });
  }
  return needs;
}

/**
 * One Limits control's change. Field names are the controls' `name` attributes; a tool checkbox
 * passes its tool as `value` and whether it is now checked.
 */
export function changeTargetLimits(
  draft: TargetLimitsDraft,
  field: string,
  value: string | boolean,
  checked?: boolean,
): TargetLimitsDraft {
  if (field === 'limits.enabled' && typeof value === 'boolean') return { ...draft, enabled: value };
  if (field === 'limits.machine_override' && typeof value === 'boolean') return { ...draft, machineOverride: value };
  if (field === 'limits.tools' && typeof value === 'string' && checked !== undefined) {
    // An unchecked tool drops its models; checked again, it starts with any model.
    const models = Object.fromEntries(Object.entries(draft.models).filter(([tool]) => checked || tool !== value));
    return {
      ...draft,
      tools: checked
        ? draft.tools.includes(value)
          ? draft.tools
          : [...draft.tools, value]
        : draft.tools.filter((tool) => tool !== value),
      models,
    };
  }
  if (field === 'limits.role_sessions' && typeof value === 'string' && checked !== undefined) {
    const role = ROLE_SESSIONS.find((known) => known === value);
    if (!role) return draft;
    const chosen = checked ? [...draft.roleSessions, role] : draft.roleSessions.filter((item) => item !== role);
    const roleSessions = ROLE_SESSIONS.filter((known) => chosen.includes(known));
    // HS2-R2N33M: both checked is what Auto does, so it is stored as Auto (no entry).
    return { ...draft, roleSessions, roleSessionsAuto: roleSessions.length === ROLE_SESSIONS.length };
  }
  if (typeof value !== 'string') return draft;
  if (field === 'limits.max_sessions') return { ...draft, max_sessions: value };
  if (field === 'limits.resources') return { ...draft, resources: value };
  const modelsOf = /^limits\.models\.(.+)$/u.exec(field)?.[1];
  if (modelsOf && draft.tools.includes(modelsOf)) return { ...draft, models: { ...draft.models, [modelsOf]: value } };
  if (field === 'limits.build_ahead' && (value === 'default' || value === 'on' || value === 'off'))
    return { ...draft, buildAhead: value };
  const machineField = /^limits\.machine\.(.+)$/u.exec(field)?.[1];
  if (machineField && (MACHINE_FIELDS as readonly string[]).includes(machineField))
    return { ...draft, machine: { ...draft.machine, [machineField]: value } };
  return draft;
}

/**
 * The policy to store with one project's entry and its required resources replaced: every other
 * project, resource entry and member round-trips unchanged. The project's tools may not be ones the
 * policy turns off (HS2-N3HWP9; an unlisted tool is Auto), their models and the resources must be what
 * the server accepts, and its session cap may not exceed its machine's.
 */
export function policyWithTargetLimits(
  view: OrchestrationPolicyView | undefined,
  draft: TargetLimitsDraft,
): { policy: OrchestrationPolicyBody } | { error: string } {
  const body: OrchestrationPolicyBody = structuredClone(view?.policy ?? {}),
    previous = targetOf(body, draft.targetId);
  // HS2-N5BDE4: an empty Sessions Here is Auto, stored by leaving the member out; 0 starts none here.
  const sessions = draft.max_sessions.trim() ? whole(draft.max_sessions, 'Sessions Here', 0, 64) : undefined;
  if (typeof sessions === 'string') return { error: sessions };
  const off = draft.tools.find((name) => toolOff(body.tools?.find((tool) => tool.tool === name)));
  if (off) return { error: `${off} is turned off in the policy's tools.` };
  const scopes: NonNullable<OrchestrationTargetPolicy['tools']> = [];
  for (const tool of draft.tools) {
    const models = parseModels(draft.models[tool] ?? '', `${tool} models`);
    if (typeof models === 'string') return { error: models };
    scopes.push(models.length ? { tool, models } : { tool });
  }
  const resources = parseResources(draft.resources);
  if (typeof resources === 'string') return { error: resources };
  let machine: OrchestrationTargetPolicy['machine'];
  if (draft.machineOverride) {
    const limits: Record<string, number> = {};
    for (const field of MACHINE_FIELDS) {
      const parsed = whole(
        draft.machine[field],
        LABELS[field],
        field === 'max_sessions' || field === 'max_cpu_percent' ? 1 : 0,
        field === 'max_sessions' ? 64 : field === 'max_cpu_percent' ? 100 : undefined,
      );
      if (typeof parsed === 'string') return { error: parsed };
      limits[field] = parsed;
    }
    machine = limits;
  }
  const machineCap = machine?.max_sessions ?? body.machine?.max_sessions ?? POLICY_DEFAULTS.machine.max_sessions;
  if (sessions !== undefined && sessions > machineCap)
    return { error: `Sessions Here must be at most ${String(machineCap)}, the machine's session limit.` };
  const entry: OrchestrationTargetPolicy = {
    ...previous,
    target_id: draft.targetId,
    enabled: draft.enabled,
    tools: scopes,
  };
  if (sessions === undefined) delete entry.max_sessions;
  else entry.max_sessions = sessions;
  if (!scopes.length) delete entry.tools;
  if (machine) entry.machine = machine;
  else delete entry.machine;
  if (draft.buildAhead === 'default') delete entry.speculation;
  else entry.speculation = draft.buildAhead === 'on';
  // The retired actions list is never written back (HS2-XHQ1EH).
  delete entry.actions;
  const targets = body.targets ?? [];
  body.targets = previous
    ? targets.map((item) => (item.target_id === draft.targetId ? entry : item))
    : [...targets, entry];
  // HS2-YSY7AR: this project's required resources; other projects' entries stay as stored.
  const required = body.placement?.resources ?? [],
    kept = required.filter((item) => item.target_id !== draft.targetId),
    mine = resources.length ? [{ target_id: draft.targetId, resources }] : [],
    at = required.findIndex((item) => item.target_id === draft.targetId),
    next = at < 0 ? [...kept, ...mine] : [...required.slice(0, at), ...mine, ...required.slice(at + 1)];
  if (next.length || body.placement?.resources) {
    body.placement = { ...body.placement };
    if (next.length) body.placement.resources = next;
    else delete body.placement.resources;
  }
  // HS2-TK9ADC: this project's role sessions, replaced in place; other projects' stay. HS2-R2N33M: Auto is
  // no entry; a choice is kept, even of none.
  const opted = body.placement?.role_sessions ?? [],
    roleEntry = draft.roleSessionsAuto ? [] : [{ target_id: draft.targetId, roles: [...draft.roleSessions] }],
    roleAt = opted.findIndex((item) => item.target_id === draft.targetId),
    roles =
      roleAt < 0 ? [...opted, ...roleEntry] : [...opted.slice(0, roleAt), ...roleEntry, ...opted.slice(roleAt + 1)];
  if (roles.length || body.placement?.role_sessions) {
    body.placement = { ...body.placement };
    if (roles.length) body.placement.role_sessions = roles;
    else delete body.placement.role_sessions;
  }
  return { policy: body };
}

/* ---- Models and resources as chosen values (HS2-1X1TXC, HS2-NPAZQW) ---------------------- */

/** The draft field a chosen-values control edits: `limits.resources` or `limits.models.<tool>`. */
function listField(
  draft: TargetLimitsDraft,
  field: string,
): { read: string; write: (text: string) => TargetLimitsDraft } | undefined {
  if (field === 'limits.resources') return { read: draft.resources, write: (resources) => ({ ...draft, resources }) };
  const tool = /^limits\.models\.(.+)$/u.exec(field)?.[1];
  if (tool && draft.tools.includes(tool))
    return {
      read: draft.models[tool] ?? '',
      write: (text) => ({ ...draft, models: { ...draft.models, [tool]: text } }),
    };
  return undefined;
}

/** Add one chosen model or resource (or pattern); a blank or repeated value changes nothing. */
export function addTargetLimitsValue(draft: TargetLimitsDraft, field: string, value: string): TargetLimitsDraft {
  const list = listField(draft, field),
    added = listOf(value);
  if (!list || !added.length) return draft;
  const values = listOf(list.read);
  const next = [...values, ...added.filter((item, index) => !values.includes(item) && added.indexOf(item) === index)];
  return next.length === values.length ? draft : list.write(next.join(', '));
}

/** Remove one chosen model or resource. */
export function removeTargetLimitsValue(draft: TargetLimitsDraft, field: string, value: string): TargetLimitsDraft {
  const list = listField(draft, field);
  if (!list) return draft;
  const values = listOf(list.read);
  return values.includes(value) ? list.write(values.filter((item) => item !== value).join(', ')) : draft;
}

/**
 * HS2-1X1TXC: a model family as a pattern: the name up to its first part that starts with a digit,
 * then `*` (`claude-opus-4-1` → `claude-opus-*`, `gpt-5-codex` → `gpt-*`). A name without a version
 * part (an alias such as `opus`) has no family.
 */
export function modelFamily(model: string): string | undefined {
  const parts = model.split('-'),
    at = parts.findIndex((part, index) => index > 0 && /^\d/u.test(part));
  return at > 0 ? `${parts.slice(0, at).join('-')}-*` : undefined;
}

/**
 * HS2-1X1TXC: what a tool's Models control offers: each family pattern of the models the tool
 * reports, then the models themselves, less those already chosen.
 */
export function modelSuggestions(known: readonly string[], chosen: readonly string[]): string[] {
  const families = [...new Set(known.map(modelFamily).filter((family): family is string => Boolean(family)))].sort();
  return [...new Set([...families, ...known])].filter((model) => !chosen.includes(model));
}

/**
 * HS2-NPAZQW: what Required Resources offers: the declared resources (every exact resource the policy
 * requires of any project and every resource someone holds on this project's machine), a `<namespace>/*`
 * pattern for each of their namespaces, less those already chosen.
 */
export function resourceSuggestions(
  body: OrchestrationPolicyBody | null | undefined,
  held: readonly { namespace: string; resource: string }[],
  chosen: readonly string[],
): string[] {
  const declared = [...(body?.placement?.resources ?? []).flatMap((entry) => entry.resources), ...held]
      .map((need) => `${need.namespace}/${need.resource}`)
      .filter((name) => !name.includes('*')),
    exact = [...new Set(declared)].sort(),
    patterns = [...new Set(exact.map((name) => `${name.split('/')[0] ?? ''}/*`))].sort();
  return [...patterns, ...exact].filter((name) => !chosen.includes(name));
}
