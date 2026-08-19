// Introspection helper: dump the HS1 schema (tables + columns) so we can write the
// exporter against the real column names. Opens a COPY of the datadir — never the
// live one. Usage: node src/introspect.mjs <path-to-.hotsheet/db>
import { PGlite } from '@electric-sql/pglite';
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node src/introspect.mjs <path-to-.hotsheet/db>');
  process.exit(2);
}

const work = mkdtempSync(join(tmpdir(), 'hs1-introspect-'));
cpSync(source, work, { recursive: true });

const db = new PGlite(work);
try {
  const tables = await db.query(`
    select table_name
    from information_schema.tables
    where table_schema = 'public'
    order by table_name
  `);
  for (const { table_name } of tables.rows) {
    const cols = await db.query(
      `select column_name, data_type, is_nullable
       from information_schema.columns
       where table_schema = 'public' and table_name = $1
       order by ordinal_position`,
      [table_name],
    );
    const count = await db.query(`select count(*)::int as n from "${table_name}"`);
    console.log(`\n== ${table_name} (${count.rows[0].n} rows) ==`);
    for (const c of cols.rows) {
      console.log(`   ${c.column_name.padEnd(24)} ${c.data_type}${c.is_nullable === 'YES' ? ' NULL' : ''}`);
    }
  }
} finally {
  await db.close();
  rmSync(work, { recursive: true, force: true });
}
