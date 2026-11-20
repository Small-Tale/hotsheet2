/**
 * Synthetic fixtures and an in-memory orchestration API for the Project Type and Orchestration
 * settings demos (HS2-W27N1D). The production controller runs over it: no network request, no timer.
 * Names and machines are invented; no customer content.
 */
import type {
  OrchestrationBinding,
  OrchestrationCapacity,
  OrchestrationManifest,
  OrchestrationPolicyView,
  OrchestrationSession,
  OrchestrationToolCapacity,
  ProjectType,
} from '../api';
import { ApiHttpError } from '../api';
import { createOrchestrationSettings, type OrchestrationApi } from '../features/orchestration-settings';

export const DEMO_OWNER = 'launch-0000000000ab';
export const DEMO_NOW = 1_794_000_000_000;
export const DEMO_REMOTE = '01JSYNTHREMTE0000000000001';
const SERVER = '01JSYNTHSERVER000000000001';
export const DEMO_TARGETS = {
  api: '01JSYNTHTARGET000000000001',
  web: '01JSYNTHTARGET000000000002',
  docs: '01JSYNTHTARGET000000000003',
} as const;

export const DEMO_OPEN_PROJECTS = [
  { id: DEMO_OWNER, name: 'Launch' },
  { id: `remote:${DEMO_REMOTE}:api-0000000000ab`, name: 'API' },
  { id: 'web-0000000000ab', name: 'Web' },
  { id: 'docs-0000000000ab', name: 'Docs' },
  { id: `remote:${DEMO_REMOTE}:mobile-0000000000ab`, name: 'Mobile' },
] as const;

/** HS2-92FJXM: checkouts the serving machine has open; web-0000000000ab is already added. */
export const DEMO_HOSTED_CHECKOUTS = [
  { checkout_id: 'web-0000000000ab', name: 'Web' },
  { checkout_id: 'billing-0000000000ab', name: 'Billing' },
  { checkout_id: 'docs-0000000000ab', name: 'Docs' },
] as const;

export const DEMO_REMOTE_LABELS = { [DEMO_REMOTE]: 'Build Host' };

export const demoTime = (ms: number) => {
  const minutes = Math.round((DEMO_NOW - ms) / 60_000);
  return minutes <= 0
    ? 'now'
    : minutes < 60
      ? `${String(minutes)} min ago`
      : `${String(Math.round(minutes / 60))} h ago`;
};

export const demoToolLabel = (tool: string) => ({ claude: 'Claude', codex: 'Codex' })[tool] ?? tool;

/**
 * HS2-E9VVQF: the synthetic ticket store each checkout reaches. Mobile is another clone of the API
 * project, so it reaches the API's store; every other checkout has a store of its own.
 */
const DEMO_STORES: Readonly<Record<string, string>> = { api: 'a', mobile: 'a', web: 'b', docs: 'd', billing: 'e' };
export function demoStore(checkoutId: string): string {
  return (DEMO_STORES[checkoutId.split('-')[0]] ?? 'f').repeat(64);
}

export function demoBinding(target: keyof typeof DEMO_TARGETS): OrchestrationBinding {
  const checkout = `${target}-0000000000ab`;
  return {
    revision: 1,
    destination:
      target === 'api'
        ? {
            kind: 'remote',
            registration_id: DEMO_REMOTE,
            identity: { server_id: SERVER },
            checkout_id: checkout,
            checkout_authority: demoStore(checkout),
          }
        : { kind: 'local', server_id: SERVER, checkout_id: checkout, checkout_authority: demoStore(checkout) },
    checked_at_ms: DEMO_NOW - 120_000,
    confirmed_at_ms: DEMO_NOW - 120_000,
    last_failure: null,
    stale: true,
  };
}

export const demoCapacity: OrchestrationCapacity = {
  sampled_at_ms: DEMO_NOW,
  memory: { free_mb: 9_216, total_mb: 32_768 },
  cpu: { load_percent: 38, logical_cpus: 12 },
  disk: { free_mb: 210_000, total_mb: 512_000 },
  tools: [
    { tool: 'claude', installed: true, configured: true },
    { tool: 'codex', installed: true, configured: true },
  ],
  sessions: [{ session_id: 'synthetic-terminal-1', tool: 'claude', busy: true }],
  claims: [{ namespace: 'vm', resource: 'macos-vm', expires_at_ms: DEMO_NOW + 900_000 }],
  claim_namespaces: ['vm'],
  // HS2-ZY4Z4G: a synthetic machine.
  capabilities: {
    os: 'linux',
    os_version: '24.04',
    os_distribution: 'ubuntu',
    arch: 'arm64',
    cpu_model: 'Synthetic 12-core CPU',
    gpu: { present: true, vendors: ['nvidia'] },
    ai_tools: [
      { tool: 'claude', version: '2.0.14' },
      { tool: 'codex', version: '0.46.0' },
    ],
    toolchains: [
      { name: 'node', installed: true, version: '22.11.0' },
      { name: 'cargo', installed: true, version: '1.97.0' },
      { name: 'docker', installed: false },
    ],
  },
};

export const demoToolCapacity: OrchestrationToolCapacity = {
  enabled: true,
  now_ms: DEMO_NOW,
  tools: [
    { tool: 'claude', state: 'available', verified: true, max_sessions: 2, weight: 50 },
    { tool: 'codex', state: 'limited', until_ms: DEMO_NOW + 3_600_000, verified: true, max_sessions: 3, weight: 60 },
  ],
};

export const demoSessions: OrchestrationSession[] = [
  {
    session_id: '01JSYNTHSESSION00000000001',
    target_id: DEMO_TARGETS.web,
    role: 'worker',
    tool: 'claude',
    state: 'running',
    owner: 'orchestrator',
    paused: false,
    hosted: { server_id: SERVER, session_id: 'synthetic-terminal-1', worker_id: 'claude-synthetic' },
    label: 'Claude · Launch run',
    created_at_ms: DEMO_NOW - 1_800_000,
    updated_at_ms: DEMO_NOW - 60_000,
  },
  {
    session_id: '01JSYNTHSESSION00000000002',
    target_id: DEMO_TARGETS.web,
    role: 'integrator',
    tool: 'codex',
    state: 'running',
    owner: 'human',
    paused: true,
    created_at_ms: DEMO_NOW - 7_200_000,
    updated_at_ms: DEMO_NOW - 600_000,
  },
];

function fixtureManifest(): OrchestrationManifest {
  return {
    enabled: true,
    revision: '01JSYNTHREVISION0000000001',
    targets: [
      { target_id: DEMO_TARGETS.api, state: 'active', label: 'API' },
      { target_id: DEMO_TARGETS.web, state: 'active', label: 'Web' },
      { target_id: DEMO_TARGETS.docs, state: 'active', label: 'Docs' },
    ],
  };
}

function fixturePolicy(): OrchestrationPolicyView {
  return {
    enabled: true,
    revision: '01JSYNTHREVISION0000000002',
    policy: {
      machine: { max_sessions: 2, min_free_ram_mb: 2048, max_cpu_percent: 90, min_free_disk_mb: 5120 },
      tools: [
        { tool: 'codex', max_sessions: 3, weight: 60 },
        { tool: 'claude', max_sessions: 2, weight: 50 },
      ],
      // API is not listed: no session may start there, so admission is not asked (HS2-Y3N3PH).
      targets: [
        { target_id: DEMO_TARGETS.web, tools: [{ tool: 'codex' }, { tool: 'claude' }], max_sessions: 2 },
        { target_id: DEMO_TARGETS.docs, tools: [{ tool: 'codex' }, { tool: 'claude' }], max_sessions: 1 },
      ],
      wind_down: { idle_minutes: 15, keep_warm: 0 },
      speculation: { allowed: false },
      placement: {
        automatic: true,
        actor: 'synthetic-person',
        resources: [{ target_id: DEMO_TARGETS.web, resources: [{ namespace: 'vm', resource: 'macos-vm' }] }],
      },
    },
  };
}

/** An in-memory orchestration project. `empty` starts it with no projects added. */
export function createDemoOrchestrationApi({ empty = false }: { empty?: boolean } = {}) {
  let projectType: ProjectType = 'orchestration';
  let manifest = empty ? { ...fixtureManifest(), targets: [] } : fixtureManifest();
  let bindings: Record<string, OrchestrationBinding> = empty
    ? {}
    : {
        [DEMO_TARGETS.api]: demoBinding('api'),
        [DEMO_TARGETS.web]: demoBinding('web'),
        [DEMO_TARGETS.docs]: demoBinding('docs'),
      };
  let policy = fixturePolicy();
  let sessions = demoSessions.map((session) => ({ ...session }));
  let counter = 10;
  const active = () => manifest.targets.filter((target) => target.state === 'active').length;
  const typeView = (changed = false) => ({
    project_type: projectType,
    declared: true,
    active_projects: active(),
    changed,
  });
  const resolution = (target: string) => {
    const declaration = manifest.targets.find((item) => item.target_id === target)!,
      unreachable = target === DEMO_TARGETS.docs;
    return {
      declaration,
      binding: bindings[target],
      state: unreachable ? ('unreachable' as const) : ('reachable' as const),
      reason: unreachable ? 'unreachable' : null,
      stale: false,
      usable: !unreachable,
    };
  };
  const api: OrchestrationApi = {
    projectType: () => Promise.resolve(typeView()),
    setProjectType: (_owner, next) => {
      const changed = next !== projectType;
      projectType = next;
      return Promise.resolve(typeView(changed));
    },
    orchestrationTargets: () => Promise.resolve(structuredClone(manifest)),
    orchestrationBinding: (_owner, target) =>
      Promise.resolve(
        bindings[target] ?? {
          revision: 0,
          destination: null,
          checked_at_ms: null,
          confirmed_at_ms: null,
          last_failure: null,
          stale: true,
        },
      ),
    orchestrationResolve: (_owner, target) => Promise.resolve(resolution(target)),
    // HS2-D4G81Q: each project with its sites; every bound site reads as the synthetic machine.
    orchestrationSites: () => {
      const live = manifest.targets.filter((target) => target.state === 'active'),
        bound = new Map(Object.entries(bindings)),
        site = (target: (typeof live)[number]) => {
          const binding = bound.get(target.target_id);
          return {
            target_id: target.target_id,
            label: target.label,
            project: !target.site_of,
            destination: binding?.destination ?? null,
            confirmed_at_ms: binding?.confirmed_at_ms ?? null,
            last_failure: null,
            stale: true,
            capabilities: binding?.destination ? demoCapacity.capabilities : undefined,
          };
        };
      return Promise.resolve({
        revision: manifest.revision,
        projects: live
          .filter((target) => !target.site_of)
          .map((project) => ({
            project_id: project.target_id,
            label: project.label,
            sites: [project, ...live.filter((target) => target.site_of === project.target_id)].map(site),
          })),
      });
    },
    // HS2-E9VVQF: which synthetic store an open project reaches; binds nothing.
    inspectOrchestrationTarget: (_owner, destination) =>
      Promise.resolve({
        destination:
          destination.kind === 'remote'
            ? {
                ...destination,
                identity: { server_id: SERVER },
                checkout_authority: demoStore(destination.checkout_id),
              }
            : { ...destination, server_id: SERVER, checkout_authority: demoStore(destination.checkout_id) },
        stale: false,
        usable: true,
      }),
    separateOrchestrationSite: (_owner, target, expectedRevision) => {
      if (expectedRevision !== manifest.revision)
        return Promise.reject(new ApiHttpError('orchestration declaration changed', 409, 'conflict'));
      counter += 1;
      manifest = {
        ...manifest,
        revision: `01JSYNTHREVISION00000000${String(counter)}`,
        targets: manifest.targets.map((item) => (item.target_id === target ? { ...item, site_of: undefined } : item)),
      };
      return Promise.resolve(structuredClone(manifest));
    },
    addOrchestrationProject: (_owner, request) => {
      const existing = Object.entries(bindings).find(
        ([, binding]) => binding.destination?.checkout_id === request.destination.checkout_id,
      );
      // HS2-E9VVQF: a site must reach the store of its project's bound sites, as the server checks.
      const projectStore = request.site_of
        ? Object.entries(bindings).find(
            ([id]) =>
              id === request.site_of ||
              manifest.targets.some((item) => item.target_id === id && item.site_of === request.site_of),
          )?.[1].destination?.checkout_authority
        : undefined;
      if (!existing && projectStore && projectStore !== demoStore(request.destination.checkout_id))
        return Promise.reject(
          new ApiHttpError('the site is bound to another ticket store than its project', 409, 'site_store_mismatch'),
        );
      if (existing)
        return Promise.resolve({
          target_id: existing[0],
          label: request.label,
          added: false,
          resolution: resolution(existing[0]),
        });
      counter += 1;
      const id = `01JSYNTHTARGET0000000000${String(counter)}`;
      manifest = {
        ...manifest,
        targets: [
          ...manifest.targets,
          {
            target_id: id,
            state: 'active',
            label: request.label,
            ...(request.site_of ? { site_of: request.site_of } : {}),
          },
        ],
      };
      bindings = {
        ...bindings,
        [id]: {
          ...demoBinding('web'),
          destination:
            request.destination.kind === 'remote'
              ? {
                  kind: 'remote',
                  registration_id: request.destination.registration_id,
                  identity: { server_id: SERVER },
                  checkout_id: request.destination.checkout_id,
                  checkout_authority: demoStore(request.destination.checkout_id),
                }
              : {
                  kind: 'local',
                  server_id: SERVER,
                  checkout_id: request.destination.checkout_id,
                  checkout_authority: demoStore(request.destination.checkout_id),
                },
        },
      };
      return Promise.resolve({ target_id: id, label: request.label, added: true, resolution: resolution(id) });
    },
    // HS2-92FJXM: the serving machine's open checkouts, for an orchestration project open from it.
    hostedOrchestrationProjects: () =>
      Promise.resolve(
        DEMO_HOSTED_CHECKOUTS.map(({ checkout_id, name }) => {
          const bound = Object.entries(bindings).find(
            ([, binding]) => binding.destination?.kind === 'local' && binding.destination.checkout_id === checkout_id,
          );
          return { checkout_id, name, added: Boolean(bound), ...(bound ? { target_id: bound[0] } : {}) };
        }),
      ),
    addHostedOrchestrationProject: (owner, request) =>
      api.addOrchestrationProject(owner, {
        destination: { kind: 'local', checkout_id: request.checkout_id },
        label: request.label,
        target_id: request.target_id,
      }),
    removeHostedOrchestrationProject: (owner, target) => api.removeOrchestrationProject(owner, target),
    removeOrchestrationProject: (_owner, target) => {
      manifest = {
        ...manifest,
        targets: manifest.targets.map((item) => (item.target_id === target ? { ...item, state: 'removed' } : item)),
      };
      bindings = Object.fromEntries(Object.entries(bindings).filter(([id]) => id !== target));
      return Promise.resolve({ target_id: target, unbound: true, removed: true });
    },
    orchestrationCapacity: (_owner, target) =>
      target === DEMO_TARGETS.docs
        ? Promise.reject(new ApiHttpError('orchestration observation failed: unreachable', 502, 'unreachable'))
        : Promise.resolve(
            target === DEMO_TARGETS.web ? demoCapacity : { ...demoCapacity, claims: null, claim_namespaces: [] },
          ),
    orchestrationAdmission: (_owner, _target, tool) =>
      Promise.resolve(
        tool === 'codex'
          ? { outcome: 'reduced', tool, requested: 2, admitted: 1, reason: 'session_cap_reached' }
          : { outcome: 'accepted', tool, requested: 1, admitted: 1, reason: null },
      ),
    orchestrationToolCapacity: () => Promise.resolve(demoToolCapacity),
    orchestrationSessions: () => Promise.resolve(sessions.map((session) => ({ ...session }))),
    stopOrchestrationSession: (_owner, id, force) => {
      const session = sessions.find((item) => item.session_id === id)!;
      if (!force && session.role === 'worker')
        return Promise.reject(new ApiHttpError('orchestration session refused the request', 409, 'session_not_safe'));
      sessions = sessions.filter((item) => item.session_id !== id);
      return Promise.resolve({ ...session, state: 'stopped' });
    },
    takeOverOrchestrationSession: (_owner, id) => {
      sessions = sessions.map((item) => (item.session_id === id ? { ...item, owner: 'human' } : item));
      return Promise.resolve(sessions.find((item) => item.session_id === id)!);
    },
    orchestrationPolicy: () => Promise.resolve(structuredClone(policy)),
    setOrchestrationPolicy: (_owner, _revision, body) => {
      policy = { enabled: true, revision: `01JSYNTHREVISION00000000${String((counter += 1))}`, policy: body };
      return Promise.resolve(structuredClone(policy));
    },
  };
  return { api };
}

/** The production controller over an in-memory project, with deterministic confirmations. */
export function createDemoOrchestrationSettings(options: { empty?: boolean } = {}) {
  let demo = createDemoOrchestrationApi(options);
  const settings = createOrchestrationSettings({
    api: () => demo.api,
    openProjects: () => DEMO_OPEN_PROJECTS,
    confirm: () => Promise.resolve(true),
    // HS2-E9VVQF: a duplicate from + Add Project is added as a site, deterministically.
    choose: () => Promise.resolve('confirm'),
    openProjectPicker: () => undefined,
    openFlowBusy: () => false,
    activateProject: () => undefined,
    showTerminal: () => true,
    hasTerminal: (_project, terminal) => terminal === 'synthetic-terminal-1',
    knownTools: () => ['claude', 'codex'],
    showInspector: () => undefined,
    defer: (run) => {
      run();
    },
  });
  settings.remoteLabels.value = DEMO_REMOTE_LABELS;
  return {
    settings,
    reset: async () => {
      demo = createDemoOrchestrationApi(options);
      settings.closeDetail(DEMO_OWNER);
      settings.settings.value = { ...settings.settings.value, [DEMO_OWNER]: undefined };
      await settings.loadType(DEMO_OWNER);
      await settings.loadSettings(DEMO_OWNER);
    },
  };
}
