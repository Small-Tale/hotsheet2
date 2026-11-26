#!/usr/bin/env node
// Introspection helper: dump the HS1 schema (tables + columns) so we can write the
// exporter against the real column names. Opens a COPY of the datadir — never the
// live one — with the exporter's own database probe, so a 0.3.x cluster (tables in
// `template1`) lists its tables too (HS2-Q77A53).
// Usage: node src/introspect.mjs <path-to-.hotsheet/db>
import nodeFs from 'node:fs';
import nodeOs from 'node:os';
import nodePath from 'node:path';

import { copyDatabase, errorText, isDirectEntry, openCluster } from './export.mjs';

export const INTROSPECT_USAGE = 'usage: node src/introspect.mjs <path-to-.hotsheet/db>';

/**
 * Print every public table of the HS1 cluster at `source` (a `.hotsheet/db`
 * directory) with its row count and columns, through `log`. Copies the datadir to a
 * fresh temp directory under `tmpRoot` and removes it on every path, including a copy
 * or open failure. Returns `{ database, tables }` for callers and tests.
 * @param {string} source
 * @param {{ log?: (line: string) => void, tmpRoot?: string }} [options]
 */
export async function introspectDatadir(source, { log = console.log, tmpRoot = nodeOs.tmpdir() } = {}) {
  const work = nodeFs.mkdtempSync(nodePath.join(tmpRoot, 'hs1-introspect-'));
  try {
    copyDatabase(nodeFs, nodePath, source, work);
    const { db, database } = await openCluster(work);
    try {
      log(`(database: ${database})`);
      const tables = [];
      const { rows } = await db.query(`
        select table_name
        from information_schema.tables
        where table_schema = 'public'
        order by table_name
      `);
      for (const { table_name } of rows) {
        const cols = await db.query(
          `select column_name, data_type, is_nullable
           from information_schema.columns
           where table_schema = 'public' and table_name = $1
           order by ordinal_position`,
          [table_name],
        );
        const count = await db.query(`select count(*)::int as n from "${table_name.replaceAll('"', '""')}"`);
        log(`\n== ${table_name} (${count.rows[0].n} rows) ==`);
        for (const c of cols.rows) {
          log(`   ${c.column_name.padEnd(24)} ${c.data_type}${c.is_nullable === 'YES' ? ' NULL' : ''}`);
        }
        tables.push({ name: table_name, rows: count.rows[0].n, columns: cols.rows.map((c) => c.column_name) });
      }
      return { database, tables };
    } finally {
      await db.close();
    }
  } finally {
    nodeFs.rmSync(work, { recursive: true, force: true });
  }
}

async function main(argv) {
  const args = argv.slice(2);
  if (args.length !== 1 || args[0].startsWith('-')) {
    console.error(INTROSPECT_USAGE);
    process.exitCode = 2;
    return;
  }
  await introspectDatadir(args[0]);
}

if (isDirectEntry(import.meta.url, process.argv[1])) {
  main(process.argv).catch((err) => {
    console.error(`hotsheet-introspect: ${errorText(err)}`);
    process.exitCode = 1;
  });
}
