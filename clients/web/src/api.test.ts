import { describe,expect,it } from 'vitest';

import { encodeAttachmentFilename } from './api';

describe('attachment filename transport',()=>{
  it('encodes macOS screenshot names as an ASCII-safe header value',()=>{
    const encoded=encodeAttachmentFilename('Screenshot 2026-08-31 at 8.49.09 AM.png');
    expect(encoded).toBe('Screenshot%202026-08-31%20at%208.49.09%E2%80%AFAM.png');
    expect(new TextEncoder().encode(encoded)).toHaveLength(encoded.length);
  });
});
