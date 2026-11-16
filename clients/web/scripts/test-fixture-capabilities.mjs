import { spawnSync } from 'node:child_process';

import { launchOnce } from './first-launch.mjs';

/** HS2-SFVWN5: verify the actual artifact before any fixture state or credentials open. */
export function assertFixtureServer(binary, execute = spawnSync, warm = launchOnce) {
  assertCapability(binary, true, execute, warm);
}

export function assertFixtureCli(binary, execute = spawnSync, warm = launchOnce) {
  assertCapability(binary, false, execute, warm);
}

function assertCapability(binary, server, execute, warm) {
  // The bounded probe below is often a fresh binary's first launch; launch it once untimed first,
  // so the macOS first-launch assessment cannot spend the 5 s bound (HS2-04D6DW).
  warm(binary);
  const result = execute(binary, ['--test-fixture-capabilities'], {
    encoding: 'utf8',
    timeout: 5000,
    maxBuffer: 64 * 1024,
  });
  let capability;
  try {
    capability = JSON.parse(result.stdout ?? '');
  } catch {
    // A stale binary without the probe is not an isolated fixture.
  }
  if (
    result.error ||
    result.status !== 0 ||
    (server && capability?.isolated_permission_vault !== true) ||
    capability?.isolated_native_credentials !== true
  )
    throw new Error(
      'HS2-SFVWN5: unattended fixtures require an isolated debug server artifact. Build once with ' +
        'cargo build -p hotsheet-cli -p hotsheet-server --bins --features hotsheet-aitools/test-permission-vault. ' +
        'Temporary HOTSHEET_HOME does not isolate native credentials.',
    );
}
