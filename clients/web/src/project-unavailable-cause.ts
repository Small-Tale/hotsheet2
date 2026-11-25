/**
 * Plain-language explanation of why a remembered project could not be reopened (HS2-ZKAVZZ). The
 * bridge reports a raw error such as `ENOENT: no such file or directory, realpath '/…/project'`; this
 * module classifies it into a known cause, a title-style title, a sentence-style explanation, and the
 * action that fits best. The raw error stays available as technical detail for diagnosis.
 *
 * Titles share the wording of the HS2-H0QGK7 region empty states ("Project Unavailable",
 * "Server Unavailable") where the cause is the same.
 */
export type ProjectUnavailableCause =
  'folder-missing' | 'not-a-folder' | 'permission-denied' | 'server-unreachable' | 'store-link-broken' | 'unknown';

export interface ProjectUnavailableExplanation {
  cause: ProjectUnavailableCause;
  /** Title-style EmptyState title (a fragment, no ending punctuation). */
  title: string;
  /** Sentence-style explanation and next step. */
  detail: string;
  /** The action that fits the cause: Retry for causes that may pass, Close Tab when the folder is gone. */
  primaryAction: 'retry' | 'close';
}

export interface ProjectUnavailableFailure {
  root: string;
  name: string;
  /** The raw error the bridge or client reported. */
  error: string;
  /** A registered server process that is not responding. */
  recoveryPid?: number;
}

/** A Node file-system error: `CODE: description, syscall 'path'`. */
const FS_ERROR = /\b(ENOENT|ENOTDIR|EACCES|EPERM)\b(?:[^']*'([^']+)')?/;

const SERVER_UNREACHABLE = [
  /\bserver (?:is )?unavailable\b/i,
  /\bnot responding\b/i,
  /\bdid not answer\b/i,
  /\bE(?:CONNREFUSED|CONNRESET|HOSTUNREACH|TIMEDOUT)\b/,
  /\bfetch failed\b/i,
  /\bfailed to fetch\b/i,
  /^load failed$/i,
  /\bnetworkerror\b/i,
  /\bsocket hang up\b/i,
  /\btimed out waiting for a healthy hot sheet server\b/i,
];

const STORE_LINK_BROKEN = [
  /\blinked ticket repository is unavailable\b/i,
  /\bno hot sheet 2 ticket repository was found\b/i,
  /\bticket repository is no longer available\b/i,
];

/** macOS reports `/var/…` roots as `/private/var/…` after `realpath`; compare paths without it. */
function comparablePath(path: string): string {
  const unified = path.replace(/\\/g, '/').replace(/\/+$/, '');
  return unified.replace(/^\/private(?=\/(?:var|tmp|etc)\b)/, '');
}

/** Where a file-system error's path sits relative to the project folder. */
function pathRelation(path: string | undefined, root: string): 'root' | 'ancestor' | 'inside' | 'outside' | 'none' {
  if (!path) return 'none';
  const target = comparablePath(path),
    folder = comparablePath(root);
  if (target === folder) return 'root';
  if (folder.startsWith(`${target}/`)) return 'ancestor';
  if (target.startsWith(`${folder}/`)) return 'inside';
  return 'outside';
}

/** Classify a raw restore error into a known cause; `unknown` when nothing matches. */
export function projectUnavailableCause(
  failure: Pick<ProjectUnavailableFailure, 'root' | 'error' | 'recoveryPid'>,
): ProjectUnavailableCause {
  const error = failure.error.trim();
  if (STORE_LINK_BROKEN.some((pattern) => pattern.test(error))) return 'store-link-broken';
  const fs = FS_ERROR.exec(error);
  if (fs) {
    const [, code, path] = fs,
      relation = pathRelation(path, failure.root);
    if (code === 'EACCES' || code === 'EPERM') return 'permission-denied';
    // The project path, or a folder above it, is a file: opening `<root>/.hotsheet2/store` fails with ENOTDIR.
    if (code === 'ENOTDIR') return relation === 'outside' ? 'store-link-broken' : 'not-a-folder';
    if (relation === 'root' || relation === 'ancestor' || relation === 'none') return 'folder-missing';
    // A missing path outside the project is the ticket repository it links to.
    if (relation === 'outside') return 'store-link-broken';
    return 'unknown';
  }
  if (failure.recoveryPid || SERVER_UNREACHABLE.some((pattern) => pattern.test(error))) return 'server-unreachable';
  return 'unknown';
}

/** The plain-language title, explanation, and best action for a failed project restore. */
export function explainProjectUnavailable(failure: ProjectUnavailableFailure): ProjectUnavailableExplanation {
  const cause = projectUnavailableCause(failure),
    { name } = failure;
  switch (cause) {
    case 'folder-missing':
      return {
        cause,
        title: 'Project Folder Not Found',
        detail: `The folder for ${name} was moved, renamed, or deleted. Put it back and choose Retry, or close the tab and open the project from its new location.`,
        primaryAction: 'close',
      };
    case 'not-a-folder':
      return {
        cause,
        title: 'Project Path Is Not a Folder',
        detail: `The location remembered for ${name} is now a file, not a folder. Close the tab and open the project from its folder.`,
        primaryAction: 'close',
      };
    case 'permission-denied':
      return {
        cause,
        title: 'No Permission to Open Project',
        detail: `Hot Sheet is not allowed to read the folder for ${name}. Allow access to the folder, then choose Retry.`,
        primaryAction: 'retry',
      };
    case 'server-unreachable':
      return {
        cause,
        title: 'Server Unavailable',
        detail: `The local Hot Sheet server is not responding, so ${name} cannot open. The tab stays here; choose Retry once the server is running. If it keeps failing, quit and relaunch Hot Sheet.`,
        primaryAction: 'retry',
      };
    case 'store-link-broken':
      return {
        cause,
        title: 'Ticket Repository Not Found',
        detail: `The ticket repository that ${name} links to was moved or deleted. Restore the repository and choose Retry, or close the tab and open the project again to choose its tickets.`,
        primaryAction: 'retry',
      };
    case 'unknown':
      return {
        cause,
        title: 'Project Unavailable',
        detail: `Hot Sheet could not open ${name}. Choose Retry; if it keeps failing, the technical details below say what went wrong.`,
        primaryAction: 'retry',
      };
    default:
      return cause satisfies never;
  }
}

/** The raw diagnosis lines kept behind the Technical Details disclosure. */
export function projectUnavailableTechnicalDetails(failure: ProjectUnavailableFailure): string[] {
  return [
    failure.error.trim() || 'No error was reported.',
    ...(failure.recoveryPid ? [`Registered Hot Sheet server process ${failure.recoveryPid} is not responding.`] : []),
    `Project folder: ${failure.root}`,
  ];
}
