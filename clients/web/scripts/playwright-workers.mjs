/**
 * Local Playwright parallelism that respects how busy the machine already is (HS2-MHPHZB).
 *
 * Every worker drives a browser against one shared Vite dev server, so the suite is CPU-bound on a
 * single machine. Playwright's default (half the cores) is fine on an idle machine, but when other
 * builds or agents already saturate it, extra workers only stretch each test past its timeout. Start
 * from half the cores and give one worker back for every core's worth of existing load beyond the
 * idle half, never dropping below three workers (or half the cores on a smaller machine). CI and an explicit override keep full control.
 *
 * @param {{ cpus: number, load1: number, override?: string, ci?: boolean }} input
 * @returns {number | undefined} a worker count, or undefined to use Playwright's default
 */
export function localPlaywrightWorkers({ cpus, load1, override, ci = false }) {
  const requested = Number.parseInt(override ?? '', 10);
  if (Number.isFinite(requested) && requested > 0) return requested;
  if (ci) return undefined;
  const half = Math.max(1, Math.floor(cpus / 2)),
    excess = Math.max(0, Math.floor(load1 - cpus / 2));
  return Math.max(Math.min(3, half), half - excess);
}
