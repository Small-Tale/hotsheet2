import { spawnSync } from 'node:child_process';
import { statSync } from 'node:fs';
import { resolve } from 'node:path';

const launched = new Set();

/**
 * Launch a freshly built or copied executable once, without any deadline, before a test times it
 * (HS2-04D6DW, HS2-W00050). macOS assesses a newly linked or newly copied executable on its first
 * launch through one queue for the whole host; while other worktrees build and run their own fresh
 * binaries that wait can exceed a test's whole startup window. After this launch a busy host only
 * slows the test. `args` must exit at once without side effects. Each file (path and inode, so a
 * replaced copy counts as new) launches once per process; a failed launch is ignored, because the
 * caller's own probe reports a missing binary. The Rust integration tests share the same helper in
 * `crates/test-support/first_launch.rs`.
 */
export function launchOnce(binary, args = ['--version'], execute = spawnSync) {
  let inode = 'missing';
  try {
    inode = String(statSync(binary).ino);
  } catch {
    // The launch below fails the same way; the caller's probe names the missing binary.
  }
  const key = `${resolve(binary)}\0${inode}`;
  if (launched.has(key)) return binary;
  execute(binary, args, { stdio: 'ignore' });
  launched.add(key);
  return binary;
}
