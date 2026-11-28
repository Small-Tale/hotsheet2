// HS2-3R7DK6: select changed Playwright cases from complete statements on both sides of the diff.
import { createRequire } from 'node:module';
import { changedLines } from './platform-cfg.mjs';

const require = createRequire(import.meta.url);

function testStatements(ts, path, text) {
  const source = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  if (source.parseDiagnostics.length) return null;
  const starts = source.getLineStarts();
  const lines = new Map();
  const titles = new Set();

  function visit(node) {
    if (ts.isExpressionStatement(node)) {
      const call = node.expression;
      if (ts.isCallExpression(call) && ts.isIdentifier(call.expression) && call.expression.text === 'test') {
        const [name] = call.arguments;
        // A stable prefix covers all cases emitted by a parameterized test.
        let title;
        if (name && ts.isStringLiteralLike(name)) title = name.text;
        if (name && ts.isTemplateExpression(name) && name.head.text.length >= 12) title = name.head.text;
        const from = node.getStart(source);
        const to = node.getEnd();
        const start = source.getLineAndCharacterOfPosition(from).line + 1;
        const end = source.getLineAndCharacterOfPosition(to - 1).line + 1;
        const before = text.slice(starts[start - 1], from).trim();
        const after = text.slice(to, starts[end] ?? text.length).trim();
        // Sharing a line with other code makes a line-level diff ambiguous.
        if (title && !before && (!after || after.startsWith('//'))) {
          const nameLine = source.getLineAndCharacterOfPosition(name.getStart(source)).line + 1;
          const scope = { title, nameLine, start, end, argumentsAfterTitle: text.slice(name.getEnd(), to) };
          titles.add(title);
          for (let line = start; line <= end; line += 1) lines.set(line, scope);
        }
        return;
      }
    }
    ts.forEachChild(node, visit);
  }

  visit(source);
  return { lines, titles };
}

export function changedBrowserTitles(path, before, after, diff) {
  if (after === null || diff === null) return [];
  let ts;
  try {
    ts = require('../clients/web/node_modules/typescript');
  } catch (error) {
    if (error.code !== 'MODULE_NOT_FOUND') throw error;
    return [];
  }
  const old = testStatements(ts, path, before ?? '');
  const next = testStatements(ts, path, after);
  if (!old || !next) return [];

  const addedTitles = new Set();
  const removedTitles = new Set();
  const replacements = new Set();
  for (const hunk of diff.split(/(?=^@@ )/m).slice(1)) {
    const { removed, added } = changedLines(hunk);
    const oldScopes = new Set(removed.map((line) => old.lines.get(line)));
    const newScopes = new Set(added.map((line) => next.lines.get(line)));
    if (oldScopes.has(undefined) || newScopes.has(undefined)) return [];
    for (const scope of oldScopes) removedTitles.add(scope.title);
    for (const scope of newScopes) addedTitles.add(scope.title);

    // A hunk replacing a complete test with a different body may be a deletion plus an addition.
    if (oldScopes.size === 1 && newScopes.size === 1) {
      const [from] = oldScopes;
      const [to] = newScopes;
      const retainsBody =
        from.argumentsAfterTitle === to.argumentsAfterTitle || removed.length < from.end - from.start + 1;
      if (retainsBody && removed.includes(from.nameLine) && added.includes(to.nameLine)) replacements.add(from.title);
    }
  }
  for (const title of removedTitles) {
    if (next.titles.has(title)) addedTitles.add(title);
    else if (!replacements.has(title)) return [];
  }
  return [...addedTitles];
}
