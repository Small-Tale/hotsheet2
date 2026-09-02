import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import { createDevApp } from '../dev-server';
import { createFrameBatcher } from './frame-batcher';
import { clampRectToViewport, intersectRectWithViewport, normalizeRect, resizeRect, translateAnchoredRect } from './geometry';
import { createCliDevReviewSubmitter, validateDevReviewSubmission } from './server';

const capture = { id: '1', filename: '../review.png', dataUrl: `data:image/png;base64,${Buffer.from('png').toString('base64')}`, width: 10, height: 10 };
const attachment = { id: 'file-1', filename: '../notes.txt', dataUrl: `data:text/plain;base64,${Buffer.from('notes').toString('base64')}`, mimeType: 'text/plain', size: 5 };
const submission = { notes: 'Button overlaps heading', captures: [capture], attachments: [attachment], pageUrl: 'http://localhost/ux-demo', viewport: { width: 1200, height: 800 } };

describe('dev review tool', () => {
  it('coalesces bursty pointer geometry work into one animation-frame update', () => {
    let callback: FrameRequestCallback | undefined;
    const host = {
      requestAnimationFrame: vi.fn((next: FrameRequestCallback) => { callback = next; return 7; }),
      cancelAnimationFrame: vi.fn(),
    };
    const update = vi.fn();
    const batch = createFrameBatcher(host, update);
    batch.schedule(); batch.schedule(); batch.schedule();
    expect(host.requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(update).not.toHaveBeenCalled();
    callback?.(16);
    expect(update).toHaveBeenCalledTimes(1);
    batch.schedule(); batch.flush();
    expect(host.cancelAnimationFrame).toHaveBeenCalledWith(7);
    expect(update).toHaveBeenCalledTimes(2);
  });

  it('normalizes, resizes, and clamps capture geometry', () => {
    expect(normalizeRect('a', 80, 70, 20, 10)).toEqual({ id: 'a', x: 20, y: 10, width: 60, height: 60 });
    expect(resizeRect({ id: 'a', x: 20, y: 10, width: 60, height: 60 }, 'se', 100, 90)).toEqual({ id: 'a', x: 20, y: 10, width: 80, height: 80 });
    expect(resizeRect({ id: 'a', x: 20, y: 10, width: 60, height: 60 }, 'nw', 75, 65)).toEqual({ id: 'a', x: 56, y: 46, width: 24, height: 24 });
    expect(resizeRect({ id: 'a', x: 20, y: 10, width: 60, height: 60 }, 'e', 110, 40)).toEqual({ id: 'a', x: 20, y: 10, width: 90, height: 60 });
    expect(resizeRect({ id: 'a', x: 20, y: 10, width: 60, height: 60 }, 'n', 50, 30)).toEqual({ id: 'a', x: 20, y: 30, width: 60, height: 40 });
    expect(clampRectToViewport({ id: 'a', x: -5, y: 90, width: 120, height: 40 }, 100, 100)).toEqual({ id: 'a', x: 0, y: 90, width: 100, height: 10 });
    expect(intersectRectWithViewport({ id: 'a', x: -5, y: 90, width: 20, height: 20 }, 100, 100)).toEqual({ id: 'a', x: 0, y: 90, width: 15, height: 10 });
    expect(translateAnchoredRect({ id: 'a', x: 20, y: 30, width: 40, height: 50 }, { x: 100, y: 200 }, { x: 85, y: 140 })).toEqual({ id: 'a', x: 5, y: -30, width: 40, height: 50 });
  });

  it('validates notes and sanitizes capture and attachment filenames', () => {
    const validated = validateDevReviewSubmission(submission);
    expect(validated.captures[0].filename).toBe('review.png');
    expect(validated.attachments[0].filename).toBe('notes.txt');
    expect(() => validateDevReviewSubmission({ ...submission, notes: ' ' })).toThrow(/notes/);
    expect(() => validateDevReviewSubmission({ ...submission, captures: [{ ...capture, dataUrl: 'data:image/jpeg;base64,x' }] })).toThrow(/PNG/);
    expect(() => validateDevReviewSubmission({ ...submission, attachments: [{ ...attachment, dataUrl: 'not-data' }] })).toThrow(/Attachment/);
  });

  it('keeps ticket submission development-only and delegates a validated payload', async () => {
    const submit = vi.fn(async () => ({ slug: 'HS2-REVIEW' }));
    const app = createDevApp(true, submit);
    expect((await app.request('/__hotsheet/dev-review/tickets', { method: 'POST', body: JSON.stringify(submission) })).status).toBe(404);
    const response = await app.request('/__hotsheet/dev-review/tickets', { method: 'POST', headers: { 'content-type': 'application/json', 'x-hotsheet-dev-review': '1' }, body: JSON.stringify(submission) });
    expect(response.status).toBe(201);
    expect(await response.json()).toEqual({ slug: 'HS2-REVIEW' });
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({ notes: submission.notes, captures: [expect.objectContaining({ filename: 'review.png' })] }));
    expect((await createDevApp(false, submit).request('/__hotsheet/dev-review/tickets', { method: 'POST', headers: { 'x-hotsheet-dev-review': '1' } })).status).toBe(404);
  });

  it('creates through the CLI adapter and attaches every decoded PNG', async () => {
    const temp = await mkdtemp(resolve(tmpdir(), 'dev-review-test-'));
    const cli = resolve(temp, 'fake-hotsheet');
    const log = resolve(temp, 'calls.log');
    await writeFile(cli, `#!/bin/sh\nprintf '%s\\n' "$*" >> "${log}"\nif [ "$3" = "new" ]; then printf 'Created HS2-REVIEW (ticket.md)\\n'; fi\nif [ "$3" = "attach" ] && [ ! -f "$5" ]; then exit 9; fi\n`);
    await chmod(cli, 0o755);
    try {
      const finalize = vi.fn(async () => undefined);
      const result = await createCliDevReviewSubmitter({ repoRoot: temp, storePath: resolve(temp, 'store'), cliPath: cli, finalize })({ ...submission, notes: '- Cancel is too close to the countdown.' });
      expect(result).toEqual({ slug: 'HS2-REVIEW' });
      const calls = await readFile(log, 'utf8');
      expect(calls).toContain('-C');
      expect(calls).toContain('new --title=UX feedback: - Cancel is too close to the countdown.');
      expect(calls).toContain('--details=- Cancel is too close to the countdown.');
      expect(calls).toContain('attach HS2-REVIEW');
      expect(calls.match(/attach HS2-REVIEW/g)).toHaveLength(2);
      expect(finalize).toHaveBeenCalledWith(resolve(temp, 'store'), 'HS2-REVIEW');
    } finally { await rm(temp, { recursive: true, force: true }); }
  });

  it('surfaces a shell-quoted, copy-paste-runnable command when the CLI fails', async () => {
    const temp = await mkdtemp(resolve(tmpdir(), 'dev-review-fail-'));
    const cli = resolve(temp, 'fake-hotsheet');
    await writeFile(cli, `#!/bin/sh\nprintf 'boom\\n' >&2\nexit 1\n`);
    await chmod(cli, 0o755);
    try {
      const finalize = vi.fn(async () => undefined);
      const run = createCliDevReviewSubmitter({ repoRoot: temp, storePath: resolve(temp, 'my store.hs2'), cliPath: cli, finalize });
      await expect(run({ ...submission, notes: 'Cancel is too close to the countdown' })).rejects.toThrow(
        // The store path and the space-bearing --title value stay single-quoted so the
        // logged command re-parses to the same argv instead of splitting on spaces.
        /Command failed: .*'\S.*my store\.hs2'.*'--title=UX feedback: Cancel is too close to the countdown'/,
      );
      expect(finalize).not.toHaveBeenCalled();
    } finally { await rm(temp, { recursive: true, force: true }); }
  });
});
