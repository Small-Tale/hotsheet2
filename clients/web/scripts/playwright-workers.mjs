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

/** Default niceness for local Playwright runs (HS2-FVT34A). */
export const DEFAULT_PLAYWRIGHT_NICE = 10;

/**
 * The niceness local Playwright should move to, or undefined to leave the priority alone
 * (HS2-FVT34A). `PLAYWRIGHT_NICE` overrides the default; `0` opts out. CI keeps its priority
 * unless an override is given, and an already lower priority (higher niceness) is never raised,
 * which an unprivileged process could not do anyway.
 *
 * @param {{ current: number, override?: string, ci?: boolean }} input
 * @returns {number | undefined}
 */
export function playwrightNiceness({ current, override, ci = false }) {
  const requested = Number.parseInt(override ?? '', 10);
  const hasOverride = Number.isFinite(requested);
  if (ci && !hasOverride) return undefined;
  const target = Math.min(19, hasOverride ? requested : DEFAULT_PLAYWRIGHT_NICE);
  return target > 0 && target > current ? target : undefined;
}

/**
 * Lower this process's CPU priority before Playwright forks its workers, browsers, and web server,
 * which all inherit it, so several parallel suites cannot starve the desktop (HS2-FVT34A). Best
 * effort: a failure leaves the priority unchanged.
 *
 * @param {{ getPriority: () => number, setPriority: (priority: number) => void }} os
 * @param {NodeJS.ProcessEnv} env
 */
export function lowerPlaywrightPriority(os, env) {
  try {
    const target = playwrightNiceness({
      current: os.getPriority(),
      override: env.PLAYWRIGHT_NICE,
      ci: Boolean(env.CI),
    });
    if (target !== undefined) os.setPriority(target);
  } catch {
    // Unsupported platform or permission: run at the inherited priority.
  }
}
