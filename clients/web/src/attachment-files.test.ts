import { describe, expect, it } from 'vitest';

import { describeUnreadableAttachments, screenAttachmentFiles } from './attachment-files';

class UnreadableFile extends File {
  override arrayBuffer(): Promise<ArrayBuffer> { return Promise.reject(new TypeError('backing file is unavailable')); }
}

class ShortFile extends File {
  override arrayBuffer(): Promise<ArrayBuffer> { return Promise.resolve(new Uint8Array([1]).buffer); }
}

describe('attachment file ingestion', () => {
  it('materializes readable files into stable in-memory files', async () => {
    const source = new File(['proof'], 'proof.txt', { type: 'text/plain', lastModified: 42 });
    const result = await screenAttachmentFiles([source]);
    expect(result.unreadable).toEqual([]);
    expect(result.readable).toHaveLength(1);
    expect(result.readable[0]).not.toBe(source);
    expect(result.readable[0]).toMatchObject({ name: 'proof.txt', size: 5, type: 'text/plain', lastModified: 42 });
    await expect(result.readable[0].text()).resolves.toBe('proof');
  });

  it('rejects empty, throwing, and short-read promised files while preserving valid siblings', async () => {
    const result = await screenAttachmentFiles([
      new File([], 'empty.png'),
      new UnreadableFile(['image'], 'floating-capture.png'),
      new ShortFile(['image'], 'partial.png'),
      new File(['ok'], 'saved.png'),
    ]);
    expect(result.readable.map(file => file.name)).toEqual(['saved.png']);
    expect(result.unreadable).toEqual(['empty.png', 'floating-capture.png', 'partial.png']);
  });

  it('gives actionable macOS guidance instead of exposing a fetch TypeError', () => {
    expect(describeUnreadableAttachments(['floating-capture.png'])).toBe('“floating-capture.png” has no readable content yet. A new macOS screen capture can be dragged before it has been written to disk. Wait for it to appear on the desktop, then add it again.');
    expect(describeUnreadableAttachments([])).toBe('');
  });
});
