// HS2-6CK09C: a green merged report must include every declared browser shard.
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Validate all artifact names and blob files before copying any report for Playwright. */
export function verifyPlaywrightShards(artifacts, destination, count) {
  if (!Number.isSafeInteger(count) || count < 1) throw new Error('Browser shard count must be a positive integer.');
  const expected = Array.from({ length: count }, (_, index) => `playwright-blob-${index + 1}`);
  const actual = readdirSync(artifacts).sort();
  if (actual.join('\n') !== expected.slice().sort().join('\n'))
    throw new Error(
      `Incomplete browser shard reports: expected ${expected.join(', ')}; found ${actual.join(', ') || 'none'}.`,
    );
  const reports = expected.map((name, index) => {
    const report = `report-${index + 1}.zip`;
    const files = readdirSync(resolve(artifacts, name));
    if (files.length !== 1 || files[0] !== report)
      throw new Error(`Invalid ${name} artifact: expected only ${report}; found ${files.join(', ') || 'none'}.`);
    return { source: resolve(artifacts, name, report), report };
  });
  if (existsSync(destination) && readdirSync(destination).length)
    throw new Error(`Browser report destination must be empty: ${destination}`);
  mkdirSync(destination, { recursive: true });
  for (const { source, report } of reports) copyFileSync(source, resolve(destination, report));
  return reports.map(({ report }) => report);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    const [artifacts, destination, count] = process.argv.slice(2);
    verifyPlaywrightShards(resolve(artifacts), resolve(destination), Number(count));
    console.log(`Verified all ${count} browser shard reports.`);
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exitCode = 1;
  }
}
