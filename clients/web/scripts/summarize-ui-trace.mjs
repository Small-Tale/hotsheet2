import { readFileSync, writeFileSync } from 'node:fs';

const tracePath = process.argv[2];
if (!tracePath) throw new Error('Usage: node scripts/summarize-ui-trace.mjs <chromium-trace.json>');

const events = JSON.parse(readFileSync(tracePath, 'utf8')).traceEvents;
const markers = new Map();
for (const event of events) {
  const match = /^hs2-profile:(.*):(start|end)$/.exec(event.name ?? '');
  if (!match) continue;
  const pair = markers.get(match[1]) ?? {};
  pair[match[2]] = event;
  markers.set(match[1], pair);
}
const first = [...markers.values()][0]?.start;
if (!first) throw new Error('No HS2 UI profile markers in trace');
const rendererEvents = events.filter(
  (event) =>
    event.pid === first.pid &&
    event.tid === first.tid &&
    event.ph === 'X' &&
    Number.isFinite(event.ts) &&
    Number.isFinite(event.dur),
);
const clippedMs = (event, start, end) =>
  Math.max(0, Math.min(end, event.ts + event.dur) - Math.max(start, event.ts)) / 1000;
const source = (event) => {
  const url = event.args?.data?.url;
  if (!url) return event.args?.data?.functionName ?? '';
  const path = new URL(url).pathname;
  return path.includes('/src/') ? path.slice(path.indexOf('/src/')) : '<dependency>';
};
const windows = [];
for (const [name, pair] of markers) {
  if (!pair.start || !pair.end) throw new Error(`Incomplete trace marker: ${name}`);
  const start = pair.start.ts;
  const end = pair.end.ts;
  const inside = rendererEvents.filter((event) => event.ts < end && event.ts + event.dur > start);
  const sum = (eventName) =>
    Math.round(
      inside.reduce((total, event) => total + (event.name === eventName ? clippedMs(event, start, end) : 0), 0) * 10,
    ) / 10;
  const tasks = inside.filter((event) => event.name === 'RunTask');
  const functions = inside
    .filter((event) => event.name === 'FunctionCall')
    .map((event) => ({ ms: Math.round(clippedMs(event, start, end) * 10) / 10, source: source(event) }))
    .sort((a, b) => b.ms - a.ms);
  windows.push({
    name,
    wall_ms: Math.round((end - start) / 100) / 10,
    renderer_task_ms: sum('RunTask'),
    javascript_function_ms: sum('FunctionCall'),
    style_ms: sum('UpdateLayoutTree'),
    layout_ms: sum('Layout'),
    prepaint_ms: sum('PrePaint'),
    paint_ms: sum('Paint'),
    long_tasks_over_50ms: tasks.filter((event) => clippedMs(event, start, end) > 50).length,
    longest_task_ms: Math.round(Math.max(0, ...tasks.map((event) => clippedMs(event, start, end))) * 10) / 10,
    top_functions: functions.slice(0, 3),
  });
}
const output = tracePath.replace(/\.json$/, '-summary.json');
writeFileSync(output, JSON.stringify({ trace: tracePath, windows }, null, 2));
console.log(output);
