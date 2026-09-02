import { expect, type Page } from '@playwright/test';

export interface FeedbackRectangleMeasurement {
  dispatchMs: number;
  pointerMoves: number;
  geometryWrites: number;
  captureStarts: number;
  capturesDuringGesture: number;
  maxPointerMoveMs: number;
}

interface Metrics {
  pointerMoves: number;
  geometryWrites: number;
  captureStarts: number;
  captureStartsDuringGesture: number;
  maxPointerMoveMs: number;
}

export async function measureFeedbackRectangle(page: Page, start: { x: number; y: number }, end: { x: number; y: number }): Promise<FeedbackRectangleMeasurement> {
  const tool = page.locator('.hs-dev-review');
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await page.keyboard.down('Alt');
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(end.x, end.y);
  await page.mouse.up();
  await page.keyboard.up('Alt');
  const rectangle = tool.locator('.hs-dev-review__rect').first();
  await expect(rectangle).toBeVisible();

  const result = await rectangle.evaluate((node) => {
    const root = node.closest('.hs-dev-review') as HTMLElement & { performanceMetrics: Metrics };
    const before = { ...root.performanceMetrics };
    const bounds = node.getBoundingClientRect();
    const pointer = { pointerId: 91, pointerType: 'mouse', bubbles: true, cancelable: true, button: 0, buttons: 1 };
    node.dispatchEvent(new PointerEvent('pointerdown', { ...pointer, clientX: bounds.left + 20, clientY: bounds.top + 20 }));
    const startedAt = performance.now();
    for (let index = 0; index < 240; index += 1) {
      window.dispatchEvent(new PointerEvent('pointermove', { ...pointer, clientX: bounds.left + 20 + index / 8, clientY: bounds.top + 20 + index / 12 }));
    }
    window.dispatchEvent(new PointerEvent('pointerup', { ...pointer, buttons: 0, clientX: bounds.left + 50, clientY: bounds.top + 40 }));
    return { before, dispatchMs: performance.now() - startedAt };
  });

  // Formerly a 350 ms timer started a full-document html2canvas pass here. Waiting
  // past that boundary proves annotation remains idle between gestures.
  await page.waitForTimeout(450);
  const after = await tool.evaluate(root => ({ ...(root as HTMLElement & { performanceMetrics: Metrics }).performanceMetrics }));
  return {
    dispatchMs: result.dispatchMs,
    pointerMoves: after.pointerMoves - result.before.pointerMoves,
    geometryWrites: after.geometryWrites - result.before.geometryWrites,
    captureStarts: after.captureStarts - result.before.captureStarts,
    capturesDuringGesture: after.captureStartsDuringGesture - result.before.captureStartsDuringGesture,
    maxPointerMoveMs: after.maxPointerMoveMs,
  };
}

export function expectResponsiveFeedbackRectangle(measurement: FeedbackRectangleMeasurement): void {
  expect(measurement.pointerMoves).toBe(240);
  expect(measurement.geometryWrites).toBeLessThanOrEqual(2);
  expect(measurement.captureStarts).toBe(0);
  expect(measurement.capturesDuringGesture).toBe(0);
  expect(measurement.dispatchMs).toBeLessThan(100);
  expect(measurement.maxPointerMoveMs).toBeLessThan(16);
}
