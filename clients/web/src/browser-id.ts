let fallbackSequence = 0;

/** An opaque client identity, never an authentication credential. LAN HTTP exposes
 * getRandomValues but not the secure-context-only randomUUID convenience method. */
export function browserRandomId(
  source: (Pick<Crypto, 'getRandomValues'> & Partial<Pick<Crypto, 'randomUUID'>>) | null = (
    globalThis as { crypto?: Crypto }
  ).crypto ?? null,
): string {
  if (source?.randomUUID) return source.randomUUID();
  if (source) {
    const bytes = source.getRandomValues(new Uint8Array(16));
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }
  // Only legacy environments without Crypto use this fallback. The sequence keeps
  // mounts distinct within one page even when its clock/random source repeat.
  return `client-${Date.now().toString(36)}-${(++fallbackSequence).toString(36)}-${Math.random().toString(36).slice(2)}`;
}
