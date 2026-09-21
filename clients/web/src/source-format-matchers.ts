import { expect } from 'vitest';

export function sourceTokens(source: string): string {
  return source
    .replace(/[\s'"]/gu, '')
    .replace(/\/>/gu, '>')
    .replace(/0\./gu, '.')
    .replace(/\(([A-Za-z_$][\w$]*)\)=>/gu, '$1=>')
    .replace(/,([\])}])/gu, '$1')
    .replace(/;\}/gu, '}')
    .replace(/&&\(</gu, '&&<');
}

function compactPattern(pattern: RegExp): RegExp {
  const source = sourceTokens(pattern.source.replace(/(?<!\[)\\s[+*?]?/gu, ''))
    .replaceAll(';\\}', '\\}')
    .replace(/;$/u, '');
  return new RegExp(source, pattern.flags);
}

expect.extend({
  toContainSource(received: unknown, expected: string) {
    const actual = sourceTokens(String(received)),
      wanted = sourceTokens(expected).replace(/;$/u, ''),
      pass = actual.includes(wanted);
    return {
      pass,
      message: () =>
        pass
          ? `expected source not to contain ${this.utils.printExpected(expected)}`
          : `expected source to contain ${this.utils.printExpected(expected)}`,
    };
  },
  toMatchSource(received: unknown, expected: RegExp | string) {
    const actual = sourceTokens(String(received)),
      pass =
        typeof expected === 'string' ? actual.includes(sourceTokens(expected)) : compactPattern(expected).test(actual);
    return {
      pass,
      message: () =>
        pass
          ? `expected source not to match ${this.utils.printExpected(expected)}`
          : `expected source to match ${this.utils.printExpected(expected)}`,
    };
  },
});

declare module 'vitest' {
  interface Assertion<T> {
    toContainSource(expected: string): T;
    toMatchSource(expected: RegExp | string): T;
  }
}
