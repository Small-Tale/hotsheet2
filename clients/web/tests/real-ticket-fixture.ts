import { execFile } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { syntheticTicket } from '../scripts/scale-stress.mjs';
import type { RealTicketFixtureContext } from './real-ticket-server';

const execute = promisify(execFile);

/** Seed one disposable, committed Git ticket store for repeatable UI performance flows. */
export async function seedLocalGitTickets(
  fixture: RealTicketFixtureContext,
  groups: readonly {
    status: 'completed' | 'verified' | 'started' | 'not_started' | 'backlog' | 'archive';
    count: number;
  }[],
) {
  const { store, env } = fixture;
  const directories = new Set<string>();
  let index = 0;
  for (const group of groups) {
    for (let offset = 0; offset < group.count; offset += 1) {
      index += 1;
      const ticket = syntheticTicket(index);
      const directory = join(store, 'tickets', ticket.id.slice(-2));
      if (!directories.has(directory)) {
        await mkdir(directory, { recursive: true });
        directories.add(directory);
      }
      const body = ticket.body
        .replace(/^status: [a-z_]+$/mu, `status: ${group.status}`)
        .replace(/^up_next: true\n/gmu, '');
      await writeFile(join(directory, `${ticket.id}.md`), body);
    }
  }
  await execute('git', ['-C', store, 'add', '.gitattributes', '.gitignore', 'hotsheet-store.json', 'tickets'], { env });
  await execute(
    'git',
    [
      '-C',
      store,
      '-c',
      'user.name=Hot Sheet fixture',
      '-c',
      'user.email=fixture@example.invalid',
      'commit',
      '-m',
      `Seed ${index} local tickets`,
    ],
    { env },
  );
  const [{ stdout: remotes }, { stdout: changes }] = await Promise.all([
    execute('git', ['-C', store, 'remote'], { env }),
    execute('git', ['-C', store, 'status', '--porcelain'], { env }),
  ]);
  if (remotes.trim() || changes.trim())
    throw new Error(
      `The performance fixture must be a clean local Git store without a remote (remotes: ${remotes.trim() || 'none'}; changes: ${changes.trim() || 'none'}).`,
    );
  return { total: index };
}
