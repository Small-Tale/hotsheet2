// HS2-T9VXEP: the migrator suite spawns the real `hotsheet-migrate` and `hotsheet-cli`
// binaries. Hot Sheet-launched agent shells export HOTSHEET_HOME as the maintainer's real
// `~/.hotsheet2`, so every spawned binary would otherwise register checkouts and lock
// settings there. Give the whole run one throwaway machine home that workers and their
// child processes inherit, and remove it afterwards.
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export default function setup() {
  const home = mkdtempSync(join(tmpdir(), 'hs2-migrator-home-'));
  process.env.HOTSHEET_HOME = home;
  return () => rmSync(home, { recursive: true, force: true });
}
