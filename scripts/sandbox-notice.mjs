// HS2-5PTT2Y: the Claude Code local sandbox (macOS Seatbelt) denies AF_UNIX bind/connect,
// PTY spawn, and file-system watcher events. The terminal broker, terminal, server, session
// push, launch, and coordination suites need them, so a sandboxed run fails (or waits on a
// wake-up that never arrives) without any regression. Tests are not skipped there: a skipped
// suite would report success while proving nothing. Failure reports carry this notice instead.

/** True when the command runs inside the Claude Code sandbox (`SANDBOX_RUNTIME=1`). */
export function inSandbox(env = process.env) {
  return env.SANDBOX_RUNTIME === "1";
}

/** The line appended to a failure report inside the sandbox, or `undefined` outside it. */
export function sandboxFailureNotice(env = process.env) {
  if (!inSandbox(env)) return undefined;
  return (
    "  note (HS2-5PTT2Y): this ran inside the Claude Code sandbox (SANDBOX_RUNTIME=1), which denies " +
    "AF_UNIX sockets, PTYs, and file watchers. Rerun outside the sandbox before treating this failure as a regression."
  );
}
