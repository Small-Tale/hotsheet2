import { AppShell, type AppShellProps } from './app-shell';

/**
 * The configured application shell. main.tsx owns state selection and supplies the
 * rendered feature surfaces; this component owns the shell component boundary, including the
 * application root's edge-to-edge `viewport` presentation.
 */
export function MainShell(props: AppShellProps) {
  return <AppShell presentation="viewport" {...props} />;
}
