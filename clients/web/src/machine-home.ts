import { homedir, tmpdir } from 'node:os';
import { resolve } from 'node:path';

/**
 * The machine homes the Node-side bridges share with the Rust binaries (HS2-N2HB7G). Like
 * `hotsheet_ticketing::machine_home` (HS2-1D4DQF), an unset, empty, or whitespace-only variable
 * counts as unset, so a failed `HOTSHEET_HOME=$(mktemp -d)` substitution can never resolve the
 * home against the working directory. A non-empty value, relative or absolute, is used as given.
 * This is the only module that reads `HOTSHEET_HOME` or `HOTSHEET_LEGACY_HOME` from the
 * environment; `eslint-policy.test.ts` rejects direct reads elsewhere in `src/`.
 */
export type MachineHomeEnvironment = Readonly<Record<string, string | undefined>>;

/** `value`, or `undefined` when it is absent, empty, or whitespace-only. */
export function nonBlank(value: string | undefined): string | undefined {
  return value === undefined || value.trim() === '' ? undefined : value;
}

/** The current user's home directory, or the OS temp directory when it is blank. */
function userHome(home: () => string): string {
  return nonBlank(home()) ?? tmpdir();
}

/** `${HOTSHEET_HOME}`, else `<home>/.hotsheet2`, else `<temp>/.hotsheet2`. */
export function hotsheetHome(environment: MachineHomeEnvironment = process.env, home = homedir): string {
  return nonBlank(environment.HOTSHEET_HOME) ?? resolve(userHome(home), '.hotsheet2');
}

/** The Hot Sheet 1 home: `${HOTSHEET_LEGACY_HOME}`, else `<home>/.hotsheet`. */
export function hotsheetLegacyHome(environment: MachineHomeEnvironment = process.env, home = homedir): string {
  return nonBlank(environment.HOTSHEET_LEGACY_HOME) ?? resolve(userHome(home), '.hotsheet');
}
