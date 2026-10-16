/** Install development-only diagnostics. */
export async function installDevelopmentDiagnostics(): Promise<void> {
  if (!import.meta.env.DEV) return;
  const dev = await import('kerfjs/dev');
  dev.enableWarnings({ valueOnlyRerender: true, listRebind: true, invariants: 'throw' });
  // Flag slow clicks and requests against the 100 ms average / 150 ms peak budget (HS2-ZGBQ5Q).
  const { installDevLatencyMonitor } = await import('../dev-latency-entry');
  installDevLatencyMonitor();
  if (!import.meta.env.HOTSHEET_WEB_STABLE_DEV) await import('../dev-reload-diagnostics-entry');
}
