// HS2-3PX1E4: the Vitest files that launch the real target/debug hotsheet-cli, hotsheet-server,
// or hotsheet-migrate binaries. They form the `binaries` Vitest project, the only one whose
// globalSetup runs the cargo build (HS2-A0M8CM), so runs that select none of them skip it.
// `scripts/vitest-binary-suites.test.mjs` fails when a test file launches a binary but is not
// listed here.
export const BINARY_SUITES = [
  'scripts/local-host.e2e.test.mjs',
  'scripts/real-ticket-server-owner.e2e.test.mjs',
  'scripts/stable-dev.e2e.test.mjs',
  'src/checkouts-server-integration.test.ts',
  'src/hs1-backup.test.ts',
  'src/migration-server-integration.test.ts',
];

/** Files the detector flags that never launch a real binary, with the reason. */
export const NOT_BINARY_SUITES = {
  'src/migration-lock.test.ts': 'points HOTSHEET_MIGRATE_BIN at a missing path to assert the not-built error',
  'scripts/vitest-build-binaries.test.mjs': 'holds detector fixtures; its nested Vitest runs use a fake cargo',
};

/** A binary reference: a target/debug path, the real ticket-server harness, or the live-server gate. */
const BINARY_REFERENCE =
  /target\/debug\/hotsheet-|'target', 'debug'|realTicketServer|HOTSHEET_LIVE_SERVER|HOTSHEET_LIVE_REMOTE/;
/** A process launch; pure tests that only compute binary paths do not need the binaries. */
const PROCESS_LAUNCH = /\b(spawn|spawnSync|execFile|execFileAsync|execFileSync|runGitCommand)\b/;

/** Whether a test file's source both names a real binary and launches a process. */
export function launchesTestBinaries(source) {
  return BINARY_REFERENCE.test(source) && PROCESS_LAUNCH.test(source);
}
