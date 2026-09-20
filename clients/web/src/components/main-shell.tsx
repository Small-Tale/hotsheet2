import { AppShell, type AppShellProps } from './app-shell';

/**
 * The configured application shell. main.tsx owns state selection and supplies the
 * rendered feature surfaces; this component owns the shell component boundary.
 */
export function MainShell(props: AppShellProps) {
  return <AppShell {...props} />;
}
