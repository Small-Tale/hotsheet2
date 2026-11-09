// HS2-T7HVHA: the copied Node executable must run without a package manager's dylibs.
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

/** Tauri's externalBin resource names use the host target triple (HS2-KY5X6J). */
export function desktopTarget(platform = process.platform, arch = process.arch) {
  const triples = {
    darwin: { arm64: 'aarch64-apple-darwin', x64: 'x86_64-apple-darwin' },
    win32: { arm64: 'aarch64-pc-windows-msvc', x64: 'x86_64-pc-windows-msvc' },
  };
  const triple = triples[platform]?.[arch];
  if (!triple)
    throw new Error(
      `Unsupported Hot Sheet desktop resource target: ${platform}/${arch}. Expected macOS or Windows arm64/x64.`,
    );
  return { triple, extension: platform === 'win32' ? '.exe' : '' };
}

/** Pure copy plan, shared by native preparation and synthetic platform regression tests. */
export function desktopResourcePlan(root, binaries, nodeRuntime, platform = process.platform, arch = process.arch) {
  const { triple, extension } = desktopTarget(platform, arch);
  return [
    { source: nodeRuntime, destination: resolve(binaries, `node-${triple}${extension}`) },
    ...['hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate', 'hotsheet-mcp'].map((name) => ({
      source: resolve(root, 'target/release', `${name}${extension}`),
      destination: resolve(binaries, `${name}-${triple}${extension}`),
    })),
  ];
}

export function assertStandaloneNode(nodePath, linkage, platform = process.platform, run = execFileSync) {
  if (platform === 'win32') {
    if (!nodePath.toLowerCase().endsWith('.exe')) throw new Error('Desktop Node on Windows must be a .exe executable.');
    // The Windows runner uses setup-node's official binary. Execute the selected copy candidate
    // before staging; otool is macOS-only and cannot inspect a PE image (HS2-KY5X6J).
    try {
      run(nodePath, ['--version'], { encoding: 'utf8' });
    } catch {
      throw new Error('Desktop Node must start as a standalone Windows executable.');
    }
    return;
  }
  if (platform !== 'darwin') throw new Error(`Unsupported desktop Node platform: ${platform}.`);
  const listed = linkage ?? run('otool', ['-L', nodePath], { encoding: 'utf8' });
  const dependencies = listed
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
