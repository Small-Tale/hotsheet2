import { describe, expect, it } from 'vitest';

import { browserDraftFileRecord, fileFromBrowserDraftRecord } from './browser-draft-file-contract';

describe('browser draft file records (HS2-2HF3H4)', () => {
  it('round trips exact bytes and metadata, including empty files and a further edit', async () => {
    for (const bytes of [new Uint8Array(), new Uint8Array([0, 255, 17, 128])]) {
      const file = new File([bytes], 'synthetic.bin', { type: 'application/octet-stream', lastModified: 1234 }),
        record = await browserDraftFileRecord(file);
      expect(record.blob).toBeInstanceOf(ArrayBuffer);
      expect(record).toMatchObject({ name: file.name, type: file.type, size: bytes.length, lastModified: 1234 });
      const restored = fileFromBrowserDraftRecord(record);
      expect(new Uint8Array(await restored.arrayBuffer())).toEqual(bytes);
      // Pin compatibility with the reader in already-open older clients.
      expect(new Uint8Array(await new File([record.blob], record.name).arrayBuffer())).toEqual(bytes);
      expect([restored.name, restored.type, restored.lastModified, restored.size]).toEqual([
        file.name,
        file.type,
        1234,
        bytes.length,
      ]);
    }
  });
  it('reads legacy Blob and File records without changing their bytes or metadata', async () => {
    for (const blob of [new Blob(['legacy']), new File(['legacy'], 'old-name.txt')]) {
      const record = { blob, name: 'saved-name.txt', type: 'text/plain', lastModified: 5678 },
        restored = fileFromBrowserDraftRecord(record);
      expect(await restored.text()).toBe('legacy');
      expect([restored.name, restored.type, restored.lastModified, restored.size]).toEqual([
        'saved-name.txt',
        'text/plain',
        5678,
        6,
      ]);
      expect(record.blob).toBe(blob);
    }
  });
  it('rejects malformed and unknown shapes instead of converting missing bytes to an empty file', () => {
    const valid = { blob: new ArrayBuffer(2), name: 'synthetic.bin', type: '', lastModified: 0, size: 2 };
    for (const record of [
      undefined,
      null,
      {},
      { ...valid, blob: undefined },
      { ...valid, blob: new Uint8Array(2) },
      { ...valid, size: 1 },
      { ...valid, name: 4 },
      { ...valid, type: undefined },
      { ...valid, lastModified: NaN },
      { ...valid, lastModified: 1.5 },
      { blob: 'missing bytes', name: 'bad.txt', type: '', lastModified: 0 },
      { blob: new Blob(['legacy']), name: 'bad.txt', type: '', lastModified: 0, size: 2 },
    ])
      expect(() => fileFromBrowserDraftRecord(record)).toThrow();
  });
});
