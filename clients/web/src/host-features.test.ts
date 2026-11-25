import { describe, expect, it } from 'vitest';

import {
  documentHostFeatures,
  HOST_FEATURES_ENVIRONMENT,
  isUxDemoPath,
  parseHostFeatures,
  shouldBootUxDemo,
  stampHostFeatures,
} from './host-features';

const features = (argv: string[], environment: Record<string, string | undefined> = {}) => [
  ...parseHostFeatures(argv, environment),
];

describe('host feature flags (HS2-ZF70Y9)', () => {
  it('turns nothing on by default and ignores the host’s other arguments', () => {
    expect(features([])).toEqual([]);
    expect(features(['--host', '127.0.0.1', '--port', '0'])).toEqual([]);
    expect(features([], { [HOST_FEATURES_ENVIRONMENT]: '' })).toEqual([]);
    expect(features([], { [HOST_FEATURES_ENVIRONMENT]: '  ' })).toEqual([]);
  });

  it('accepts every flag spelling, repeated, alongside other arguments', () => {
    expect(features(['--feature', 'ux-demo'])).toEqual(['ux-demo']);
    expect(features(['--feature=ux-demo'])).toEqual(['ux-demo']);
    expect(features(['--feature:ux-demo'])).toEqual(['ux-demo']);
    expect(features(['--port', '0', '--feature:UX-Demo', '--feature', 'ux-demo', '--host', 'localhost'])).toEqual([
      'ux-demo',
    ]);
    expect(features(['--feature', 'ux-demo,ux-demo'])).toEqual(['ux-demo']);
  });

  it('reads the environment list and merges it with the flags', () => {
    expect(features([], { [HOST_FEATURES_ENVIRONMENT]: 'ux-demo' })).toEqual(['ux-demo']);
    expect(features([], { [HOST_FEATURES_ENVIRONMENT]: ' ux-demo , ' })).toEqual(['ux-demo']);
    expect(features(['--feature', 'ux-demo'], { [HOST_FEATURES_ENVIRONMENT]: 'ux-demo' })).toEqual(['ux-demo']);
  });

  it('rejects an unknown or missing feature name instead of ignoring it', () => {
    expect(() => features(['--feature', 'ux-dmeo'])).toThrow('Unknown Hot Sheet feature "ux-dmeo" in --feature');
    expect(() => features(['--feature:nope'])).toThrow('Unknown Hot Sheet feature "nope" in --feature:nope');
    expect(() => features([], { [HOST_FEATURES_ENVIRONMENT]: 'ux-demo,nope' })).toThrow(
      `Unknown Hot Sheet feature "nope" in ${HOST_FEATURES_ENVIRONMENT}`,
    );
    expect(() => features(['--feature'])).toThrow('--feature needs a feature name. Known features: ux-demo.');
    expect(() => features(['--feature', '--port', '0'])).toThrow('--feature needs a feature name');
    expect(() => features(['--feature='])).toThrow('--feature= names no feature');
    expect(() => features(['--feature:,'])).toThrow('--feature:, names no feature');
  });

  it('matches only the catalog route', () => {
    for (const path of ['/ux-demo', '/ux-demo/', '/ux-demo/anything']) expect(isUxDemoPath(path)).toBe(true);
    for (const path of ['/', '/ux-demos', '/ux-demo.html', '/settings/ux-demo', '/UX-DEMO'])
      expect(isUxDemoPath(path)).toBe(false);
  });

  it('boots the catalog only on its route when the host turned it on', () => {
    const on = new Set(['ux-demo'] as const),
      off = new Set<never>();
    expect(shouldBootUxDemo('/ux-demo', on)).toBe(true);
    expect(shouldBootUxDemo('/ux-demo', off)).toBe(false);
    expect(shouldBootUxDemo('/', on)).toBe(false);
    expect(shouldBootUxDemo('/projects/one', on)).toBe(false);
  });

  it('stamps, replaces, and removes the feature meta tag', () => {
    const html = '<html><head><title>Hot Sheet</title></head><body></body></html>';
    const stamped = stampHostFeatures(html, new Set(['ux-demo']));
    expect(stamped).toBe(
      '<html><head><title>Hot Sheet</title><meta name="hotsheet-features" content="ux-demo"></head><body></body></html>',
    );
    // Stamping again replaces the tag rather than adding a second one.
    expect(stampHostFeatures(stamped, new Set(['ux-demo']))).toBe(stamped);
    // A document stamped earlier loses the tag when the host has no features.
    expect(stampHostFeatures(stamped, new Set())).toBe(html);
    expect(stampHostFeatures(html, new Set())).toBe(html);
  });

  it('reads the stamped features back from a document, ignoring unknown names', () => {
    const document = (content: string | undefined) => ({
      querySelector: () => (content === undefined ? null : { getAttribute: () => content }),
    });
    expect([...documentHostFeatures(document(undefined) as unknown as Document)]).toEqual([]);
    expect([...documentHostFeatures(document('') as unknown as Document)]).toEqual([]);
    expect([...documentHostFeatures(document('ux-demo') as unknown as Document)]).toEqual(['ux-demo']);
    expect([...documentHostFeatures(document('other ux-demo') as unknown as Document)]).toEqual(['ux-demo']);
  });
});
