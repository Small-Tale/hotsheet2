/** Install development-only diagnostics. */
export async function installDevelopmentDiagnostics(): Promise<void> {
  if (!import.meta.env.DEV) return;
  const dev = await import('kerfjs/dev');
  dev.enableWarnings({ valueOnlyRerender: true, listRebind: true, invariants: 'throw' });
  if (!import.meta.env.HOTSHEET_WEB_STABLE_DEV) await import('../dev-reload-diagnostics-entry');
}
