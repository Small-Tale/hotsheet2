/**
 * Shared helpers for reading untrusted JSON as `unknown` and narrowing it with explicit checks
 * (HS2-3DA0FQ, following the HS2-313JET local-bridge parsers). Callers validate the shape they
 * rely on instead of casting `JSON.parse(..)` or `response.json()` to a type.
 */

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Parse JSON text as `unknown`; throws a SyntaxError for invalid text. */
export function parseJson(text: string): unknown {
  return JSON.parse(text) as unknown;
}

/** Parse JSON text as `unknown`, or `undefined` when it is not valid JSON. */
export function tryParseJson(text: string): unknown {
  try {
    return parseJson(text);
  } catch {
    return undefined;
  }
}

/** A response body as `unknown`, or `undefined` when it is empty or not JSON. */
export async function responseJson(response: Response): Promise<unknown> {
  return tryParseJson(await response.text().catch(() => ''));
}

/** The non-empty `error` string of a failure body, or undefined. */
export function errorMessageOf(body: unknown): string | undefined {
  return isRecord(body) && typeof body.error === 'string' && body.error ? body.error : undefined;
}

/** Every string entry of a JSON array, or an empty list for any other value. */
export function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}
