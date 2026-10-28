#!/usr/bin/env node
// HS2-A4HD9X: synthetic PGLite export performance baseline with no customer data.
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { PGlite } from '@electric-sql/pglite';
import { exportFromDb } from '../src/export.mjs';

const db = new PGlite();
const samples = [];
try {
  await db.exec(`
    create table tickets (
      id integer primary key, ticket_number text not null, title text not null,
      status text, up_next boolean, tags jsonb, notes jsonb
    );
    create table ticket_blocked_by (ticket_id integer, blocks_on_ticket_id integer);
    insert into tickets (id, ticket_number, title, status, up_next, tags, notes)
      select n, 'HS-' || n, 'Synthetic ticket ' || n, 'not_started', true, '[]', '[]'
      from generate_series(1, 500) as n;
  `);
  for (let index = 0; index < 7; index += 1) {
    const start = performance.now();
    const output = await exportFromDb(db, { name: 'Synthetic project', ticketPrefix: 'HS' });
    const ms = performance.now() - start;
    assert.equal(output.tickets.length, 500);
    assert.equal(output.tickets[0].ticket_number, 'HS-1');
    assert.equal(output.tickets[499].ticket_number, 'HS-500');
    if (index >= 2) samples.push(ms);
  }
  const sorted = samples.sort((a, b) => a - b);
  const p95 = Math.round(sorted[Math.ceil(sorted.length * 0.95) - 1]);
  const result = { ticket_count: 500, warmup: 2, samples: 5, export_p95_ms: p95, limit_ms: 500 };
  console.log(`HS2-A4HD9X migrator export performance: ${JSON.stringify(result)}`);
  assert.ok(p95 <= result.limit_ms, 'PGLite export exceeded the migrator performance baseline');
} finally {
  await db.close();
}
