/** Observe the owned server before it can exit (HS2-0BQ8J9). */
export function trackServerProcess(child) {
  const exited = new Promise((resolveExit) => child.once('exit', (code) => resolveExit(code)));
  const closed = new Promise((resolveClose) => child.once('close', (code) => resolveClose(code)));
  return { child, exited, closed };
}

/** Stop only this test's server, then wait for inherited pipes before removing its cache. */
export async function stopServerProcess({ child, closed }) {
  // HS2-0BQ8J9: esbuild inherits Vite's stderr. Parent exit does not mean its native service
  // has closed that pipe or finished writing. The taken-port child may already have exited.
  if (child.exitCode !== null || child.signalCode !== null) {
    await closed;
    return;
  }
  child.kill('SIGTERM');
  const force = setTimeout(() => child.kill('SIGKILL'), 10_000);
  try {
    await closed;
  } finally {
    clearTimeout(force);
  }
}
