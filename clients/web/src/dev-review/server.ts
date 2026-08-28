import { execFile } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, resolve } from 'node:path';
import { promisify } from 'node:util';
import type { DevReviewResult, DevReviewSubmission } from './index';

const run = promisify(execFile);
export type DevReviewSubmitter = (submission: DevReviewSubmission) => Promise<DevReviewResult>;

export function validateDevReviewSubmission(value: unknown): DevReviewSubmission {
  if (!value || typeof value !== 'object') throw new Error('Invalid feedback payload.');
  const input = value as Partial<DevReviewSubmission>;
  const notes = input.notes?.trim();
  if (!notes || notes.length > 10_000) throw new Error('Feedback notes must contain 1–10,000 characters.');
  if (!Array.isArray(input.captures) || input.captures.length > 10) throw new Error('Feedback supports at most 10 captures.');
  const captures = input.captures.map((capture, index) => {
    if (!capture || typeof capture.dataUrl !== 'string' || !capture.dataUrl.startsWith('data:image/png;base64,')) throw new Error(`Capture ${index + 1} is not a PNG.`);
    if (capture.dataUrl.length > 14_000_000) throw new Error(`Capture ${index + 1} is too large.`);
    return { ...capture, filename: basename(capture.filename || `ux-feedback-${index + 1}.png`) };
  });
  return { notes, captures, pageUrl: String(input.pageUrl ?? ''), viewport: { width: Number(input.viewport?.width ?? 0), height: Number(input.viewport?.height ?? 0) } };
}

export function createCliDevReviewSubmitter(options: { repoRoot: string; storePath?: string; cliPath?: string }): DevReviewSubmitter {
  const repoRoot = resolve(options.repoRoot);
  const storePath = resolve(options.storePath ?? `${repoRoot}.hs2`);
  const cliPath = resolve(options.cliPath ?? resolve(repoRoot, 'target/debug/hotsheet-cli'));
  return async raw => {
    const submission = validateDevReviewSubmission(raw);
    const firstLine = submission.notes.split('\n').find(line => line.trim())!.trim();
    const title = `UX feedback: ${firstLine}`.slice(0, 120);
    const details = `${submission.notes}\n\nCaptured from: ${submission.pageUrl}\nViewport: ${submission.viewport.width}×${submission.viewport.height}`;
    const created = await run(cliPath, ['-C', storePath, 'new', title, '--category', 'bug', '--priority', 'default', '--tag', 'client', '--tag', 'ux-feedback', '--details', details]);
    const slug = created.stdout.match(/Created\s+(HS2-[A-Z0-9]+)/)?.[1];
    if (!slug) throw new Error(`Hot Sheet did not return a ticket id: ${created.stdout.trim()}`);
    const temp = await mkdtemp(resolve(tmpdir(), 'hotsheet-dev-review-'));
    try {
      for (const [index, capture] of submission.captures.entries()) {
        const file = resolve(temp, capture.filename || `ux-feedback-${index + 1}.png`);
        await writeFile(file, Buffer.from(capture.dataUrl.slice('data:image/png;base64,'.length), 'base64'));
        await run(cliPath, ['-C', storePath, 'attach', slug, file]);
      }
    } finally { await rm(temp, { recursive: true, force: true }); }
    return { slug };
  };
}
