/** Limit app-root rendering during output streams while keeping conversation state current. */
export function createConversationRenderScheduler(render: () => void, intervalMs = 80) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
    render();
  };
  const schedule = () => {
    timer ??= setTimeout(flush, intervalMs);
  };
  const immediate = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    render();
  };
  return { schedule, flush, immediate };
}
