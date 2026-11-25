/**
 * The workspace shell for a selected project tab that is not ready (HS2-H0QGK7): a remembered project
 * that could not be reopened, a remote project still loading or reconnecting, or a local project that
 * is still opening. The shell keeps its sidebar, inspector, toolbar, and terminal drawer regions so the
 * layout never jumps between a working and a non-ready tab; each region shows an empty state and every
 * control that needs a working project is disabled with this reason.
 */
export type UnavailableProjectKind = 'restore-failure' | 'remote-loading' | 'remote-unavailable' | 'opening';

export interface UnavailableRegionMessage {
  /** Title-style EmptyState title. */
  title: string;
  /** Sentence-style detail. */
  detail: string;
}

export interface UnavailableProjectShell {
  kind: UnavailableProjectKind;
  /** The tooltip and accessible description of every project control the shell disables. */
  reason: string;
  sidebar: UnavailableRegionMessage;
  inspector: UnavailableRegionMessage;
  drawer: UnavailableRegionMessage;
}

function regionTitle(kind: UnavailableProjectKind): string {
  switch (kind) {
    case 'restore-failure':
      return 'Project Unavailable';
    case 'remote-unavailable':
      return 'Server Unavailable';
    case 'remote-loading':
    case 'opening':
      return 'Opening Project';
    default:
      return kind satisfies never;
  }
}

function reasonFor(kind: UnavailableProjectKind, name: string): string {
  switch (kind) {
    case 'restore-failure':
      return `Unavailable because ${name} could not be opened`;
    case 'remote-unavailable':
      return `Unavailable while ${name} cannot reach its server`;
    case 'remote-loading':
    case 'opening':
      return `Unavailable while ${name} opens`;
    default:
      return kind satisfies never;
  }
}

/** The non-ready shell's reason and per-region empty states for a selected project tab. */
export function unavailableProjectShell(kind: UnavailableProjectKind, name: string): UnavailableProjectShell {
  const title = regionTitle(kind),
    waiting = kind === 'restore-failure' ? `once ${name} opens again` : `once ${name} opens`;
  return {
    kind,
    reason: reasonFor(kind, name),
    sidebar: { title, detail: `Ticket views and commands appear here ${waiting}.` },
    inspector: { title, detail: `Ticket details appear here ${waiting}.` },
    drawer: { title, detail: `Terminals appear here ${waiting}.` },
  };
}

/** Which non-ready state a selected tab is in, or undefined when the selected project is ready. */
export function unavailableProjectKind(selection: {
  restoreFailure: boolean;
  /** A remote placeholder's phase: loading, or reconnecting/expired/revoked (its server is unavailable). */
  remotePhase?: 'loading' | 'reconnecting' | 'expired' | 'revoked';
  opening: boolean;
}): UnavailableProjectKind | undefined {
  if (selection.restoreFailure) return 'restore-failure';
  if (selection.remotePhase === 'loading') return 'remote-loading';
  if (selection.remotePhase) return 'remote-unavailable';
  if (selection.opening) return 'opening';
  return undefined;
}
