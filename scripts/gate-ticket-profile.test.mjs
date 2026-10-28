import assert from 'node:assert/strict';
import test from 'node:test';
import { chooseTicket, profileFromTags } from './gate-ticket-profile.mjs';

const ticket = (slug, tags = [], claimed_by = null, connection_id = 'git') => ({
  slug,
  tags,
  claimed_by,
  connection_id,
  native_id: slug,
  qualified_id: `${connection_id}:${slug}`,
});

test('HS2-VQ5ND5 explicit ticket outranks commit and claim; commit outranks claim', () => {
  const tickets = [ticket('HS2-A', [], 'worker'), ticket('HS2-B'), ticket('HS2-C')];
  assert.equal(
    chooseTicket({ explicit: 'HS2-C', subject: 'HS2-B change', workerId: 'worker', tickets }).ticket.slug,
    'HS2-C',
  );
  assert.equal(chooseTicket({ subject: 'HS2-B change', workerId: 'worker', tickets }).ticket.slug, 'HS2-B');
  assert.equal(
    chooseTicket({ subject: 'HS2-B change', dirty: true, workerId: 'worker', tickets }).ticket.slug,
    'HS2-A',
  );
  assert.equal(chooseTicket({ subject: 'HS2-B change', tickets }).ticket.qualified_id, 'git:HS2-B');
});

test('HS2-VQ5ND5 ambiguous or missing ticket references use the default profile', () => {
  const tickets = [ticket('HS2-A', [], 'worker'), ticket('HS2-B', [], 'worker')];
  assert.match(chooseTicket({ subject: 'HS2-A HS2-B change', workerId: 'worker', tickets }).reason, /names 2/);
  assert.match(chooseTicket({ dirty: true, workerId: 'worker', tickets }).reason, /2 live claims/);
  assert.throws(() => chooseTicket({ explicit: 'HS2-Z', tickets }), /resolved to 0/);
  assert.throws(
    () => chooseTicket({ explicit: 'HS2-A', tickets: [...tickets, ticket('HS2-A', [], null, 'other')] }),
    /resolved to 2/,
  );
});

test('HS2-VQ5ND5 combines known tags and validates tiers and budget overrides', () => {
  const profile = profileFromTags(
    ticket('HS2-A', [
      'tier:T0',
      'deep-testing',
      'testing:browser',
      'testing:platform',
      'testing:perf',
      'test-budget:15m',
    ]),
  );
  assert.equal(profile.tier, 'T0');
  assert.equal(profile.budgetMs, 900000);
  assert.equal(profile.deep && profile.browser && profile.platform && profile.perf, true);
  assert.match(profileFromTags(ticket('HS2-B')).reason, /default T1/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['tier:T0', 'tier:T2'])), /Conflicting risk tier/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['testing:future'])), /Unknown testing tag/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['test-budget:bad'])), /positive duration/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['tier:T2', 'test-budget:15m'])), /no time budget/);
});
