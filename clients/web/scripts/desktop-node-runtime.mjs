// HS2-T7HVHA: the copied Node executable must run without a package manager's dylibs.
import { execFileSync } from 'node:child_process';

export function assertStandaloneNode(
  nodePath,
  linkage = execFileSync('otool', ['-L', nodePath], { encoding: 'utf8' }),
) {
  const dependencies = linkage
    .trim()
    .split(/\r?\n/)
    .slice(1)
    .map((line) => line.trim().split(/\s+\(/)[0]);
  const external = dependencies.filter((path) => !path.startsWith('/usr/lib/') && !path.startsWith('/System/Library/'));
  if (external.length)
    throw new Error(
      `Desktop Node is linked to external libraries (${external.join(', ')}). Use a standalone Node executable such as the official Node 22 binary; set HOTSHEET_DESKTOP_NODE to its path.`,
    );
}
