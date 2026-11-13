/**
 * Synthetic, deterministic fixtures for the orchestration workspace design mock (HS2-HSMH2Q). They
 * follow the shapes of docs/26-orchestration-projects.md in reviewer-facing words. Every name is
 * invented ("Target A", "Build host 2"); nothing is read from a server, a store, or a real project.
 * The mock is design evidence for review, not an implementation of the workspace.
 */
import type { SemanticTone } from '@kerfjs/ui/badge';
import { signal } from 'kerfjs';

/** Which data the mock shows: everything working, one of each problem, or no targets yet. */
export type MockScenario = 'normal' | 'degraded' | 'empty';
/** How the Work section lists runs: the maintainer's leaning, or the comparison. */
export type MockGrouping = 'ticket' | 'flat';
/** The Orchestration view's sections (an open design question: sections or one long page). */
export type MockSection = 'work' | 'targets' | 'scheduler' | 'summary';

/** Readiness levels of a plan step (§26.15). */
export type ReadinessLevel = 'confirmed' | 'provisional' | 'blocked' | 'stale' | 'in_progress' | 'finished';
/** A rolled-up gate or validation verdict as the rows show it (§26.12, §26.13). */
export type Verdict = 'passed' | 'failed' | 'pending' | 'running' | 'unknown' | 'none';

export interface MockStep {
  id: string;
  /** The target ticket's display identity in its own project. */
  slug: string;
  title: string;
  target: string;
  level: ReadinessLevel;
  /** Why the step has its level, in words (the reason codes stay in the detail). */
  levelReason?: string;
  run?: { state: 'queued' | 'dispatched' | 'intervened' | 'completed'; worker: string; lease?: string };
  gate: Verdict;
  validation: Verdict;
  session?: string;
  /** Plan steps that wait on this one. */
  unblocks: number;
  /** The scheduler's answer for work that is not running (§26.17 explain). */
  next?: string;
  updated: string;
  paused?: boolean;
}

export interface MockOrchestrationTicket {
  slug: string;
  title: string;
  steps: MockStep[];
}

const steps = (scenario: MockScenario): MockOrchestrationTicket[] => [
  {
    slug: 'ORC-12',
    title: 'Roll out the shared export format',
    steps: [
      {
        id: 'step-ta41',
        slug: 'TA-41',
        title: 'Add export schema version 2',
        target: 'Target A',
        level: 'finished',
        run: { state: 'completed', worker: 'worker-a1' },
        gate: 'passed',
        validation: 'passed',
        unblocks: 2,
        updated: '1h ago',
      },
      {
        id: 'step-tb17',
        slug: 'TB-17',
        title: 'Read export schema version 2',
        target: 'Target B',
        level: 'in_progress',
        run: { state: 'dispatched', worker: 'worker-b2', lease: 'Lease 22 min' },
        gate: 'pending',
        validation: 'running',
        session: 'Codex on Build host 2',
        unblocks: 1,
        updated: '40s ago',
      },
      {
        id: 'step-tc9',
        slug: 'TC-9',
        title: 'Import export schema version 2',
        target: 'Target C',
        level: scenario === 'degraded' ? 'in_progress' : 'provisional',
        run:
          scenario === 'degraded'
            ? { state: 'dispatched', worker: 'worker-c1', lease: 'Lease unconfirmed' }
            : undefined,
        levelReason:
          scenario === 'degraded'
            ? 'Target C cannot be confirmed (unreachable for 12 min)'
            : 'Rests on TB-17 at 3f9c2a1 while its CI is pending',
        gate: scenario === 'degraded' ? 'unknown' : 'none',
        validation: 'none',
        unblocks: 0,
        next: scenario === 'degraded' ? undefined : 'Provisional work is not dispatched',
        updated: '6m ago',
      },
    ],
  },
  {
    slug: 'ORC-15',
    title: 'Stop duplicate sync retries between targets',
    steps: [
      {
        id: 'step-ta44',
        slug: 'TA-44',
        title: 'Retry a sync once on a conflict',
        target: 'Target A',
        level: 'in_progress',
        run: { state: 'dispatched', worker: 'worker-a1', lease: 'Lease 9 min' },
        gate: scenario === 'degraded' ? 'failed' : 'passed',
        validation: 'passed',
        session: 'Claude on Workstation 1',
        unblocks: 1,
        updated: '3m ago',
      },
      {
        id: 'step-tc11',
        slug: 'TC-11',
        title: 'Accept a retried sync',
        target: 'Target C',
        level: scenario === 'degraded' ? 'blocked' : 'confirmed',
        levelReason: scenario === 'degraded' ? 'TA-44 CI failed' : undefined,
        gate: 'none',
        validation: 'none',
        unblocks: 0,
        next: scenario === 'degraded' ? 'Not ready: a prerequisite gate failed' : 'Starts next on Target C with Codex',
        updated: '3m ago',
      },
    ],
  },
  {
    slug: 'ORC-18',
    title: 'Upgrade the build toolchain',
    steps: [
      {
        id: 'step-tb20',
        slug: 'TB-20',
        title: 'Move Target B to the new toolchain',
        target: 'Target B',
        level: 'confirmed',
        gate: 'none',
        validation: 'none',
        unblocks: 1,
        next: 'Waiting for build-cache on Build host 2 (held by TB-17 until 14:20)',
        updated: '9m ago',
      },
      {
        id: 'step-ta47',
        slug: 'TA-47',
        title: 'Move Target A to the new toolchain',
        target: 'Target A',
        level: 'confirmed',
        gate: 'none',
        validation: 'none',
        unblocks: 0,
        next: 'Waiting for your approval to start a Claude session',
        updated: '2m ago',
      },
    ],
  },
];

/** The orchestration tickets and their plan steps for a scenario; empty has none. */
export function mockTickets(scenario: MockScenario): MockOrchestrationTicket[] {
  return scenario === 'empty' ? [] : steps(scenario);
}

export function findStep(scenario: MockScenario, id: string): MockStep | undefined {
  return mockTickets(scenario)
    .flatMap((ticket) => ticket.steps)
    .find((step) => step.id === id);
}

export const LEVEL_PRESENTATION: Record<ReadinessLevel, { label: string; tone: SemanticTone }> = {
  confirmed: { label: 'Ready', tone: 'success' },
  provisional: { label: 'Provisional', tone: 'info' },
  blocked: { label: 'Blocked', tone: 'danger' },
  stale: { label: 'Stale', tone: 'warning' },
  in_progress: { label: 'In progress', tone: 'pop' },
  finished: { label: 'Finished', tone: 'neutral' },
};

export const VERDICT_PRESENTATION: Record<Exclude<Verdict, 'none'>, { label: string; tone: SemanticTone }> = {
  passed: { label: 'passed', tone: 'success' },
  failed: { label: 'failed', tone: 'danger' },
  pending: { label: 'pending', tone: 'info' },
  running: { label: 'running', tone: 'info' },
  unknown: { label: 'unknown', tone: 'warning' },
};

export interface MockTarget {
  id: string;
  label: string;
  binding: 'reachable' | 'unreachable' | 'unbound';
  server?: string;
  grants: number;
  policy: 'enabled' | 'not listed';
  sessions: string;
  admission: { answer: 'accepted' | 'reduced' | 'refused' | 'unknown'; detail: string };
}

export function mockTargets(scenario: MockScenario): MockTarget[] {
  if (scenario === 'empty') return [];
  return [
    {
      id: 'target-a',
      label: 'Target A',
      binding: 'reachable',
      server: 'Workstation 1 (this computer)',
      grants: 2,
      policy: 'enabled',
      sessions: '1 of 2 sessions',
      admission: { answer: 'accepted', detail: 'Room for 1 more session' },
    },
    {
      id: 'target-b',
      label: 'Target B',
      binding: 'reachable',
      server: 'Build host 2',
      grants: 1,
      policy: 'enabled',
      sessions: '1 of 2 sessions',
      admission: { answer: 'reduced', detail: 'Room for 0 of 1: free RAM below the floor' },
    },
    {
      id: 'target-c',
      label: 'Target C',
      binding: scenario === 'degraded' ? 'unreachable' : 'reachable',
      server: 'Build host 3',
      grants: 1,
      policy: 'enabled',
      sessions: '0 of 1 sessions',
      admission:
        scenario === 'degraded'
          ? { answer: 'unknown', detail: 'Unreachable since 14:02' }
          : { answer: 'accepted', detail: 'Room for 1 more session' },
    },
    {
      id: 'target-d',
      label: 'Target D',
      binding: 'unbound',
      grants: 0,
      policy: 'not listed',
      sessions: 'No sessions',
      admission: { answer: 'unknown', detail: 'Bind this target to a checkout first' },
    },
  ];
}

export const BINDING_PRESENTATION: Record<MockTarget['binding'], { label: string; tone: SemanticTone }> = {
  reachable: { label: 'Reachable', tone: 'success' },
  unreachable: { label: 'Unreachable', tone: 'danger' },
  unbound: { label: 'Not bound on this computer', tone: 'warning' },
};

/** Mock state; each entry's Settings writes it and Reset restores it. */
export const mockState = {
  scenario: signal<MockScenario>('normal'),
  grouping: signal<MockGrouping>('ticket'),
  section: signal<MockSection>('work'),
  selectedStep: signal('step-tb17'),
  selectedTarget: signal('target-b'),
  inspectorOpen: signal(true),
  mobile: signal(false),
  event: signal('Explore the orchestration workspace mock.'),
};

export function resetMockState(): void {
  mockState.scenario.value = 'normal';
  mockState.grouping.value = 'ticket';
  mockState.section.value = 'work';
  mockState.selectedStep.value = 'step-tb17';
  mockState.selectedTarget.value = 'target-b';
  mockState.inspectorOpen.value = true;
  mockState.event.value = 'Explore the orchestration workspace mock.';
}
