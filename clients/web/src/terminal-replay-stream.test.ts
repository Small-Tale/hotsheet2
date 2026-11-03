import { deflateRawSync } from 'node:zlib';

import { describe, expect, it, vi } from 'vitest';

import {
  createTerminalInboundSequencer,
  inflateRaw,
  parseTerminalFrameHeader,
  terminalAttachUrl,
  type TerminalInbound,
  terminalReplayCompressionSupported,
  terminalReplayMaxBytes,
} from './terminal-replay-stream';

const encoder = new TextEncoder();
function buffer(text: string): ArrayBuffer {
  return encoder.encode(text).buffer;
}
function header(kind: string, encoding: string, bytes: number): string {
  return JSON.stringify({ terminal_frame: { kind, encoding, bytes } });
}
function describeFrame(frame: TerminalInbound): string {
  if (frame.type === 'text') return `text:${frame.value}`;
  const kind = frame.header ? `${frame.header.kind}/${frame.header.encoding}` : 'live';
  return `${kind}:${new TextDecoder().decode(frame.bytes)}`;
}

/** An inflate whose results the test releases in any order. */
function controlledInflate() {
  const pending: {
    bytes: Uint8Array<ArrayBuffer>;
    resolve: (value: Uint8Array<ArrayBuffer>) => void;
    reject: () => void;
  }[] = [];
  const inflate = (bytes: Uint8Array<ArrayBuffer>) =>
    new Promise<Uint8Array<ArrayBuffer>>((resolve, reject) => {
      pending.push({
        bytes,
        resolve,
        reject: () => {
          reject(new Error('corrupt'));
        },
      });
    });
  return { pending, inflate };
}

describe('terminal replay stream (HS2-K93QHA)', () => {
  it('sizes the replay to the viewer scrollback plus one screen', () => {
    expect(terminalReplayMaxBytes(5_000, 24)).toBe(5_024 * 192);
    expect(terminalReplayMaxBytes(0, 0)).toBe(192);
  });

  it('adds the replay request to the attach URL and keeps its credential', () => {
    const url = new URL(
      terminalAttachUrl('ws://host/terminals/t/attach?secret=s&replay_encoding=gzip', {
        maxBytes: 4096.7,
        compression: false,
      }),
    );
    expect(url.searchParams.get('secret')).toBe('s');
    expect(url.searchParams.get('replay_protocol')).toBe('2');
    expect(url.searchParams.get('replay_max_bytes')).toBe('4096');
    expect(url.searchParams.has('replay_encoding')).toBe(false);
    const compressed = new URL(
      terminalAttachUrl(
        '/__hotsheet/project-api/p/terminals/t/attach',
        { maxBytes: 1, compression: true },
        'http://h/',
      ),
    );
    expect(compressed.pathname).toBe('/__hotsheet/project-api/p/terminals/t/attach');
    expect(compressed.searchParams.get('replay_encoding')).toBe('deflate-raw');
  });

  it('detects deflate-raw support, including browsers that reject the format', () => {
    expect(terminalReplayCompressionSupported({})).toBe(false);
    expect(terminalReplayCompressionSupported({ DecompressionStream })).toBe(true);
    const rejecting = function rejecting() {
      throw new TypeError('unsupported');
    } as unknown as typeof DecompressionStream;
    expect(terminalReplayCompressionSupported({ DecompressionStream: rejecting })).toBe(false);
  });

  it('parses only well-formed frame headers', () => {
    expect(parseTerminalFrameHeader(header('replay', 'deflate-raw', 5))).toEqual({
      kind: 'replay',
      encoding: 'deflate-raw',
      bytes: 5,
    });
    for (const bad of [
      '{"pty_size":{"cols":80,"rows":24}}',
      '{"terminal_frame"',
      '{"terminal_frame":[]}',
      header('later', 'identity', 1),
      header('replace', 'gzip', 1),
      header('replace', 'identity', -1),
      header('replace', 'identity', 1.5),
    ])
      expect(parseTerminalFrameHeader(bad)).toBeUndefined();
  });

  it('inflates real deflate-raw payloads', async () => {
    const original = encoder.encode('\u001b[32mhello\u001b[0m\r\n'.repeat(200));
    const compressed = new Uint8Array(deflateRawSync(original));
    expect(await inflateRaw(compressed)).toEqual(original);
  });

  it('delivers legacy and identity frames synchronously in order', () => {
    const delivered: string[] = [];
    const sequencer = createTerminalInboundSequencer((frame) => delivered.push(describeFrame(frame)), vi.fn());
    sequencer.push(buffer('legacy replay'));
    sequencer.push('{"pty_size":{"cols":80,"rows":24}}');
    sequencer.push(header('replace', 'identity', 4));
    sequencer.push(buffer('next'));
    sequencer.push(buffer('live'));
    sequencer.push(new Blob(['ignored']));
    expect(delivered).toEqual([
      'live:legacy replay',
      'text:{"pty_size":{"cols":80,"rows":24}}',
      'replace/identity:next',
      'live:live',
    ]);
  });

  it('holds every later frame behind a compressed replay, across repeated replays', async () => {
    const { pending, inflate } = controlledInflate(),
      delivered: string[] = [];
    const sequencer = createTerminalInboundSequencer((frame) => delivered.push(describeFrame(frame)), vi.fn(), inflate);
    sequencer.push(header('replay', 'deflate-raw', 6));
    sequencer.push(buffer('zipped'));
    sequencer.push('{"pty_size":{"cols":80,"rows":24}}');
    sequencer.push(buffer('live-1'));
    sequencer.push(header('replace', 'deflate-raw', 7));
    sequencer.push(buffer('zipped2'));
    sequencer.push(buffer('live-2'));
    expect(delivered).toEqual([]);
    expect(pending).toHaveLength(1);
    pending[0].resolve(encoder.encode('replay'));
    await vi.waitFor(() => {
      expect(pending).toHaveLength(2);
    });
    expect(delivered).toEqual(['replay/identity:replay', 'text:{"pty_size":{"cols":80,"rows":24}}', 'live:live-1']);
    sequencer.push(buffer('live-3'));
    pending[1].resolve(encoder.encode('replace'));
    await vi.waitFor(() => {
      expect(delivered).toHaveLength(6);
    });
    expect(delivered.slice(3)).toEqual(['replace/identity:replace', 'live:live-2', 'live:live-3']);
  });

  it('stops and reports a corrupt or wrong-length replay', async () => {
    for (const fail of ['reject', 'length'] as const) {
      const { pending, inflate } = controlledInflate(),
        delivered: string[] = [],
        onCorrupt = vi.fn();
      const sequencer = createTerminalInboundSequencer(
        (frame) => delivered.push(describeFrame(frame)),
        onCorrupt,
        inflate,
      );
      sequencer.push(header('replay', 'deflate-raw', 3));
      sequencer.push(buffer('xx'));
      sequencer.push(buffer('live'));
      if (fail === 'reject') pending[0].reject();
      else pending[0].resolve(encoder.encode('four'));
      await vi.waitFor(() => {
        expect(onCorrupt).toHaveBeenCalledTimes(1);
      });
      sequencer.push(buffer('after'));
      expect(delivered).toEqual([]);
    }
  });

  it('drops frames after dispose, including a replay still decoding', async () => {
    const { pending, inflate } = controlledInflate(),
      delivered: string[] = [],
      onCorrupt = vi.fn();
    const sequencer = createTerminalInboundSequencer(
      (frame) => delivered.push(describeFrame(frame)),
      onCorrupt,
      inflate,
    );
    sequencer.push(header('replay', 'deflate-raw', 6));
    sequencer.push(buffer('zipped'));
    sequencer.dispose();
    pending[0].resolve(encoder.encode('replay'));
    sequencer.push(buffer('live'));
    await Promise.resolve();
    expect(delivered).toEqual([]);
    expect(onCorrupt).not.toHaveBeenCalled();
  });
});
