import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const roles = ['dispatcher', 'integrator', 'worker'];
const write = process.argv.slice(2).includes('--write');
if (process.argv.slice(2).some((argument) => !['--write', '--check'].includes(argument))) {
  throw new Error('Use --write or --check');
}

for (const role of roles) {
  const name = `hotsheet-${role}`;
  const source = fs.readFileSync(path.join(root, 'plugins/shared/role-skills', `${name}.md`), 'utf8');
  if (!source.startsWith(`---\nname: ${name}\n`) || !/<!-- hotsheet-skill-version: \d+ -->/.test(source)) {
    throw new Error(`Invalid ${name} source`);
  }
  const claude = source.replace(/^description: ([^\n]+)\n/m, (_line, description) => {
    return `description: ${description}\nallowed-tools: Read, Grep, Glob, Edit, Write, Bash\n`;
  });
  const destinations = [
    [`.agents/skills/${name}/SKILL.md`, source],
    [`plugins/codex/${name}.md`, source],
    [`.claude/skills/${name}/SKILL.md`, claude],
    [`plugins/claude/${name}.md`, claude],
  ];
  for (const [relative, expected] of destinations) {
    const destination = path.join(root, relative);
    if (write) {
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.writeFileSync(destination, expected);
    } else if (!fs.existsSync(destination) || fs.readFileSync(destination, 'utf8') !== expected) {
      throw new Error(`Role skill drift: ${relative}; run sync-role-skills.mjs --write`);
    }
  }
}
console.log(`Verified ${roles.length} role skills and both tool mirrors`);
