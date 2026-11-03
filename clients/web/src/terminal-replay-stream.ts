/**
 * The opt-in attach replay protocol (HS2-K93QHA).
 *
 * A viewer adds `replay_protocol=2` to its attach URL with the most replay it can keep and the
 * encodings it decodes. An opted-in server announces each replay with a
 * `{"terminal_frame":{kind, encoding, bytes}}` text frame before its binary payload. A
 * compressed payload decodes asynchronously, so later frames wait behind it and the viewer
 * still applies replay and live output in server order (HS2-5W0V9M). An older server ignores
 * the parameters; its unannounced binary frames are identity payloads.
 */

export const TERMINAL_REPLAY_PROTOCOL = 2;
/** How much of a large first replay a remote viewer asks to receive first (phase 4). */
export const TERMINAL_REPLAY_TAIL_BYTES = 32 * 1024;
/** The server's own history-to-replay rule (`REPLAY_BYTES_PER_LINE`, HS2-Y5DYC0). */
export const TERMINAL_REPLAY_BYTES_PER_LINE = 192;

export type TerminalReplayKind = 'replay' | 'replace' | 'resume' | 'tail';
export type TerminalReplayEncoding = 'identity' | 'deflate-raw';

export interface TerminalFrameHeader {
  kind: TerminalReplayKind;
  encoding: TerminalReplayEncoding;
  /** Uncompressed payload length. */
  bytes: number;
  /** The output-stream position just past the payload, when the terminal reports one. */
  position?: TerminalStreamPosition;
}

/** A point in one PTY's output stream: its id and the count of bytes it has produced. */
export interface TerminalStreamPosition {
  stream: string;
  offset: number;
}

export type TerminalInbound =
  { type: 'text'; value: string } | { type: 'bytes'; bytes: Uint8Array<ArrayBuffer>; header?: TerminalFrameHeader };

export interface TerminalReplayRequest {
  maxBytes: number;
  compression: boolean;
  /** On a reconnect: the stream position this viewer has applied output up to. */
  resumeFrom?: TerminalStreamPosition;
  /** Ask for this much of a large first replay ahead of the whole replay. */
  tailBytes?: number;
}

export type InflateRaw = (bytes: Uint8Array<ArrayBuffer>) => Promise<Uint8Array<ArrayBuffer>>;

/** The most replay a viewer can show: its scrollback plus one screen, at the server's line rule. */
export function terminalReplayMaxBytes(scrollbackLines: number, rows: number): number {
  return Math.max(1, Math.ceil(scrollbackLines + rows)) * TERMINAL_REPLAY_BYTES_PER_LINE;
}

/** Whether this browser can decode `deflate-raw` (Safari 16.4+, Chrome 103+, Firefox 113+). */
export function terminalReplayCompressionSupported(
  scope: { DecompressionStream?: typeof DecompressionStream } = globalThis,
): boolean {
  const decompression = scope.DecompressionStream;
  if (typeof decompression !== 'function') return false;
  try {
    new decompression('deflate-raw');
    return true;
  } catch {
    return false;
  }
}

/** The attach URL with this viewer's replay request. */
export function terminalAttachUrl(url: string, request: TerminalReplayRequest, base?: string): string {
  const attach = new URL(url, base ?? (typeof location === 'undefined' ? undefined : location.href));
  attach.searchParams.set('replay_protocol', String(TERMINAL_REPLAY_PROTOCOL));
  attach.searchParams.set('replay_max_bytes', String(Math.max(1, Math.floor(request.maxBytes))));
  if (request.compression) attach.searchParams.set('replay_encoding', 'deflate-raw');
  else attach.searchParams.delete('replay_encoding');
  if (request.resumeFrom !== undefined)
    attach.searchParams.set('resume_from', `${request.resumeFrom.stream}:${String(request.resumeFrom.offset)}`);
  else attach.searchParams.delete('resume_from');
  if (request.tailBytes !== undefined) attach.searchParams.set('replay_tail_bytes', String(request.tailBytes));
  else attach.searchParams.delete('replay_tail_bytes');
  return attach.toString();
}

/**
 * Whether a viewer should ask for its replay tail first: only across a real network. A loopback
 * host (the desktop app, a local browser) parses the whole replay faster than it would paint a
 * tail and then parse the whole replay again.
 */
export function terminalProgressiveReplayDefault(url: string, base?: string): boolean {
  let hostname: string;
  try {
    hostname = new URL(url, base ?? (typeof location === 'undefined' ? undefined : location.href)).hostname;
  } catch {
    return false;
  }
  return !['localhost', '127.0.0.1', '[::1]', '::1'].includes(hostname) && !hostname.endsWith('.localhost');
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** The header of an announced replay, or `undefined` for any other text frame. */
export function parseTerminalFrameHeader(value: string): TerminalFrameHeader | undefined {
  if (!value.startsWith('{"terminal_frame"')) return undefined;
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return undefined;
  }
  if (!isRecord(parsed) || !isRecord(parsed.terminal_frame)) return undefined;
  const { kind, encoding, bytes, stream, end_offset: offset } = parsed.terminal_frame;
  if (kind !== 'replay' && kind !== 'replace' && kind !== 'resume' && kind !== 'tail') return undefined;
  if (encoding !== 'identity' && encoding !== 'deflate-raw') return undefined;
  if (!isCount(bytes)) return undefined;
  if (stream === undefined && offset === undefined) return { kind, encoding, bytes };
  if (typeof stream !== 'string' || !/^[0-9a-f]{16}$/.test(stream) || !isCount(offset)) return undefined;
  return { kind, encoding, bytes, position: { stream, offset } };
}

function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

/**
 * The stream position after applying `frame` (HS2-K93QHA): an announced replay sets it (unknown
 * for an older broker); live output advances a known position by its length.
 */
export function nextTerminalStreamPosition(
  current: TerminalStreamPosition | undefined,
  frame: { bytes: Uint8Array; header?: TerminalFrameHeader },
): TerminalStreamPosition | undefined {
  if (frame.header) return frame.header.position;
  return current === undefined ? undefined : { stream: current.stream, offset: current.offset + frame.bytes.length };
}

export const inflateRaw: InflateRaw = async (bytes) => {
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
};

export interface TerminalInboundSequencer {
  push(data: unknown): void;
  dispose(): void;
}

/**
 * Deliver one socket's frames in order. A header is held for the next binary frame; a
 * compressed payload is decoded before anything after it is delivered. A payload that cannot
 * be decoded, or decodes to the wrong length, reports `onCorrupt` and stops delivery, because
 * the viewer's emulator can no longer match the server.
 */
export function createTerminalInboundSequencer(
  deliver: (frame: TerminalInbound) => void,
  onCorrupt: () => void,
  inflate: InflateRaw = inflateRaw,
): TerminalInboundSequencer {
  const queue: unknown[] = [];
  let header: TerminalFrameHeader | undefined,
    decoding = false,
    stopped = false;
  const drain = () => {
    while (!decoding && !stopped && queue.length > 0) {
      const data = queue.shift();
      if (typeof data === 'string') {
        const announced = parseTerminalFrameHeader(data);
        if (announced) header = announced;
        else deliver({ type: 'text', value: data });
        continue;
      }
      if (!(data instanceof ArrayBuffer)) continue;
      const bytes = new Uint8Array(data),
        current = header;
      header = undefined;
      if (current?.encoding !== 'deflate-raw') {
        deliver(current ? { type: 'bytes', bytes, header: current } : { type: 'bytes', bytes });
        continue;
      }
      decoding = true;
      inflate(bytes).then(
        (decoded) => {
          if (stopped) return;
          decoding = false;
          if (decoded.length !== current.bytes) {
            stopped = true;
            onCorrupt();
            return;
          }
          deliver({ type: 'bytes', bytes: decoded, header: { ...current, encoding: 'identity' } });
          drain();
        },
        () => {
          if (stopped) return;
          stopped = true;
          onCorrupt();
        },
      );
    }
  };
  return {
    push(data) {
      if (stopped) return;
      queue.push(data);
      drain();
    },
    dispose() {
      stopped = true;
      queue.length = 0;
    },
  };
}
