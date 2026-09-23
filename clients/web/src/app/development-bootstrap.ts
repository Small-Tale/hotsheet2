import type { DevReviewSubmission } from '../dev-review';
import { devReviewRequested } from '../dev-review/request';
import { isMobileViewport } from '../mobile-layout';
import type { UiStabilityDiagnostics } from '../ui-stability-diagnostics';

async function submitDevReview(submission: DevReviewSubmission) {
  const response = await fetch('/__hotsheet/dev-review/tickets', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-hotsheet-dev-review': '1' },
      body: JSON.stringify(submission),
    }),
    result = (await response.json()) as { slug?: string; error?: string };
  if (!response.ok || !result.slug) throw new Error(result.error ?? 'Ticket creation failed.');
  return { slug: result.slug };
}

/** Install development-only diagnostics and return the render-stability observer, when enabled. */
export async function installDevelopmentDiagnostics(): Promise<UiStabilityDiagnostics | undefined> {
  if (!import.meta.env.DEV) return undefined;
  const dev = await import('kerfjs/dev');
  dev.enableWarnings({ valueOnlyRerender: true, listRebind: true, invariants: 'throw' });
  if (!import.meta.env.HOTSHEET_WEB_STABLE_DEV) await import('../dev-reload-diagnostics-entry');
  let diagnostics: UiStabilityDiagnostics | undefined;
  if (devReviewRequested(location.href, true)) {
    const [{ installUiStabilityDiagnostics }, devReview] = await Promise.all([
      import('../ui-stability-diagnostics'),
      import('../dev-review'),
    ]);
    diagnostics = installUiStabilityDiagnostics({
      onThrash: async (diagnostic) => {
        await submitDevReview({
          notes: 'UI stability diagnostics detected repeated unexpected control dismissal or render thrashing.',
          captures: [],
          attachments: [diagnostic],
          actorRole: 'system',
          pageUrl: location.href,
          viewport: { width: innerWidth, height: innerHeight },
        });
      },
    });
    let devReviewOverlay: { destroy(): void } | undefined;
    const syncDevReviewOverlay = () => {
      if (isMobileViewport(window.innerWidth)) {
        devReviewOverlay?.destroy();
        devReviewOverlay = undefined;
      } else {
        devReviewOverlay ??= devReview.installDevReview({
          submit: submitDevReview,
          diagnostics: () => diagnostics!.attachment(),
        });
      }
    };
    syncDevReviewOverlay();
    window.addEventListener('resize', syncDevReviewOverlay);
  }
  (
    window as typeof window & { __hotsheetUiStabilityDiagnostics?: UiStabilityDiagnostics }
  ).__hotsheetUiStabilityDiagnostics = diagnostics;
  return diagnostics;
}
