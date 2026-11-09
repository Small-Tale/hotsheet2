import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { expect, it } from 'vitest';

import { TAURI_TARGETS } from '../../../scripts/platform-matrix.mjs';
import { desktopResourcePlan, desktopTarget } from './desktop-node-runtime.mjs';

it.each([
  ['darwin', 'arm64', 'aarch64-apple-darwin', ''],
  ['darwin', 'x64', 'x86_64-apple-darwin', ''],
  ['win32', 'arm64', 'aarch64-pc-windows-msvc', '.exe'],
  ['win32', 'x64', 'x86_64-pc-windows-msvc', '.exe'],
])('HS2-KY5X6J stages %s/%s with Tauri sidecar names', (platform, arch, triple, extension) => {
  expect(desktopTarget(platform, arch)).toEqual({ triple, extension });
  const plan = desktopResourcePlan('/synthetic/root', '/synthetic/binaries', '/synthetic/node', platform, arch);
  expect(plan).toHaveLength(5);
  expect(plan[0]).toEqual({
    source: '/synthetic/node',
    destination: resolve('/synthetic/binaries', `node-${triple}${extension}`),
  });
  for (const [index, name] of ['hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate', 'hotsheet-mcp'].entries())
    expect(plan[index + 1]).toEqual({
      source: resolve('/synthetic/root', 'target/release', `${name}${extension}`),
      destination: resolve('/synthetic/binaries', `${name}-${triple}${extension}`),
    });
});

it('HS2-KY5X6J rejects unsupported desktop resource targets explicitly', () => {
  expect(() => desktopTarget('linux', 'x64')).toThrow(/Unsupported Hot Sheet desktop resource target/);
  expect(() => desktopTarget('win32', 'ia32')).toThrow(/Unsupported Hot Sheet desktop resource target/);
});

it('HS2-KY5X6J verifies staged Windows sidecars before the unsigned shell build', () => {
  const workflow = readFileSync(new URL('../../../.github/workflows/platforms.yml', import.meta.url), 'utf8');
  const prepare = workflow.indexOf('- name: Prepare the desktop resources');
  const verify = workflow.indexOf('- name: Verify Windows desktop sidecars');
  const build = workflow.indexOf('- name: Unsigned desktop build');
  expect(prepare).toBeGreaterThan(0);
  expect(verify).toBeGreaterThan(prepare);
  expect(build).toBeGreaterThan(verify);
  expect(workflow).toContain("matrix.os == 'windows' && steps.prepare.outcome == 'success'");
  expect(workflow).toContain('"$name-$triple.exe"');
  expect(workflow).toContain('"node-$triple.exe"');
  expect(workflow).toContain('matrix.bundle_args');
  expect(TAURI_TARGETS.filter((target) => target.os === 'windows').map((target) => target.bundle_args)).toEqual([
    '--no-bundle',
    '--no-bundle',
  ]);
});
