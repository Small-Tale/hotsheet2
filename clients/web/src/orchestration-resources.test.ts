import { describe, expect, it } from 'vitest';

import {
  contentionRows,
  parseContention,
  parseRequiredResources,
  resourceRows,
  type ResourceStandingView,
  resourceStateText,
} from './orchestration-resources';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

const timeline = demoTimeline('running');
const clock = (ms: number) => `t${ms}`;
const vm = (rest: Partial<ResourceStandingView> = {}): ResourceStandingView => ({
  namespace: 'vm',
  resource: 'macos-vm',
  state: 'held',
  ...rest,
});

describe('resource contention in words (HS2-5GRXFN)', () => {
  it('words every state of docs/26 §26.20, never reading an unknown as free', () => {
    const say = (rest: Partial<ResourceStandingView>) => resourceStateText(vm(rest), timeline, clock);
    expect(say({ state: 'free' })).toBe('free');
    expect(say({ expires_at_ms: 4 })).toBe('held until about t4 unless renewed');
    expect(say({})).toBe('held');
    expect(say({ state: 'taken', taken_by: 'step-3' })).toBe('given to API-12 in this pass');
    expect(say({ state: 'unreadable', unreadable: 'backend_unreachable' })).toBe(
      'its claims could not be read (backend unreachable)',
    );
    expect(say({ state: 'unreadable' })).toBe('its claims could not be read');
    expect(say({ state: 'unverified' })).toBe('its claims could not be read; work may start on a provisional claim');
    expect(say({ state: 'not_read' })).toBe('its machine was not read');
    expect(say({ state: 'new_state' })).toBe('new state');
  });

  it('lists what a card needs and each resource that is not free', () => {
    expect(resourceRows([], timeline, clock)).toEqual([]);
    expect(
      resourceRows(
        [vm({ expires_at_ms: 2 }), { namespace: 'ports', resource: '8080', state: 'free' }],
        timeline,
        clock,
      ),
    ).toEqual([
      { label: 'Needs', value: 'vm/macos-vm, ports/8080' },
      { label: 'vm/macos-vm', value: 'held until about t2 unless renewed' },
    ]);
  });

  it('lists each contended resource with who waits, one or several', () => {
    expect(
      contentionRows(
        [
          { ...vm({ expires_at_ms: 1 }), waiting: ['step-8'] },
          { ...vm({ resource: 'linux-vm', state: 'taken', taken_by: 'step-3' }), waiting: ['step-8', 'step-6'] },
          { ...vm({ resource: 'win-vm', state: 'not_read' }), waiting: [] },
        ],
        timeline,
        clock,
      ),
    ).toEqual([
      { label: 'vm/macos-vm', value: 'Held until about t1 unless renewed · DOC-9 waits' },
      { label: 'vm/linux-vm', value: 'Given to API-12 in this pass · DOC-9, API-15 wait' },
      { label: 'vm/win-vm', value: 'Its machine was not read' },
    ]);
  });

  it('parses explain’s resources and plan’s contention leniently', () => {
    expect(parseRequiredResources(undefined)).toEqual([]);
    expect(
      parseRequiredResources({
        resources: [
          { namespace: 'vm', resource: 'macos-vm', state: 'held', expires_at_ms: 3, declared_by: 'step' },
          { namespace: 'vm' },
        ],
      }),
    ).toEqual([
      {
        namespace: 'vm',
        resource: 'macos-vm',
        state: 'held',
        expires_at_ms: 3,
        unreadable: undefined,
        taken_by: undefined,
      },
    ]);
    expect(parseContention({})).toEqual([]);
    expect(
      parseContention({
        contention: [{ namespace: 'vm', resource: 'macos-vm', state: 'held', waiting: ['a', 3] }, { state: 'held' }],
      }),
    ).toEqual([
      {
        namespace: 'vm',
        resource: 'macos-vm',
        state: 'held',
        expires_at_ms: undefined,
        unreadable: undefined,
        taken_by: undefined,
        waiting: ['a'],
      },
    ]);
    expect(parseContention({ contention: [{ namespace: 'vm', resource: 'r', state: 'held' }] })[0].waiting).toEqual([]);
  });
});
