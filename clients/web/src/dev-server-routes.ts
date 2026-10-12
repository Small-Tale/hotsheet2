/** Paths handled by the Hono development bridge instead of Vite's HTML/static middleware. */
export const devServerRouteExclude =
  /^(?!\/(?:ux-demo|__hotsheet\/(?:checkouts|conversation-exports|demo-modified|folders|projects|project-api|server))(?:[/?]|$)).*/;
