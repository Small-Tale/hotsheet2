import html2canvas from 'html2canvas';
import { clampRectToViewport, normalizeRect, resizeRect, type ResizeHandle, type ReviewRect } from './geometry';
import './dev-review.css';

export interface ReviewCapture { id: string; filename: string; dataUrl: string; width: number; height: number }
export interface DevReviewSubmission { notes: string; captures: ReviewCapture[]; pageUrl: string; viewport: { width: number; height: number } }
export interface DevReviewResult { slug: string; url?: string }
export interface DevReviewOptions { submit: (submission: DevReviewSubmission) => Promise<DevReviewResult>; captureDebounceMs?: number; hintDurationMs?: number; document?: Document }
interface Selection extends ReviewRect { capture?: ReviewCapture; captureTimer?: number }
type Gesture = { kind: 'draw'; startX: number; startY: number; id: string } | { kind: 'move'; id: string; startX: number; startY: number; origin: ReviewRect } | { kind: 'resize'; id: string; handle: ResizeHandle };

export function installDevReview(options: DevReviewOptions): { destroy(): void } {
  const doc = options.document ?? document;
  const view = doc.defaultView ?? window;
  const debounceMs = options.captureDebounceMs ?? 350;
  const selections: Selection[] = [];
  let enabled = false;
  let gesture: Gesture | undefined;
  let sequence = 1;
  let selectedPreview = 0;
  let submitting = false;
  let hintTimer: number | undefined;
  let hintVisible = false;
  let modifierHeld = false;

  const root = doc.createElement('div');
  root.className = 'hs-dev-review';
  root.dataset.hotsheetDevReview = 'true';
  root.innerHTML = '<div class="hs-dev-review__toolbar"><button class="hs-dev-review__feedback" type="button" aria-pressed="false">Feedback</button></div>';
  doc.body.append(root);
  const toolbar = root.querySelector<HTMLElement>('.hs-dev-review__toolbar')!;

  const setModifierHeld = (held: boolean) => {
    modifierHeld = enabled && held;
    doc.documentElement.classList.toggle('hs-dev-review--crosshair', modifierHeld);
    root.dataset.modifierHeld = String(modifierHeld);
  };

  const scheduleHintFade = () => {
    if (hintTimer) view.clearTimeout(hintTimer);
    hintTimer = view.setTimeout(() => {
      hintTimer = undefined;
      hintVisible = false;
      root.querySelector('.hs-dev-review__hint')?.classList.add('hs-dev-review__hint--hidden');
    }, options.hintDurationMs ?? 3000);
  };

  const leaveFeedback = () => {
    if (hintTimer) { view.clearTimeout(hintTimer); hintTimer = undefined; }
    hintVisible = false;
    setModifierHeld(false);
    enabled = false;
    selections.splice(0);
    render();
  };

  const render = () => {
    toolbar.innerHTML = enabled
      ? '<button class="hs-dev-review__feedback" type="button" aria-pressed="true">Feedback</button><button data-action="new-ticket" type="button">New Ticket</button>'
      : '<button class="hs-dev-review__feedback" type="button" aria-pressed="false">Feedback</button>';
    root.querySelectorAll('.hs-dev-review__hint,.hs-dev-review__rect').forEach(node => node.remove());
    if (!enabled) return;
    const hint = doc.createElement('div');
    hint.className = hintVisible ? 'hs-dev-review__hint' : 'hs-dev-review__hint hs-dev-review__hint--hidden';
    hint.textContent = 'Hold Option/Alt and drag to capture a region';
    root.append(hint);
    selections.forEach((selection, index) => {
      const box = doc.createElement('div');
      box.className = 'hs-dev-review__rect';
      box.dataset.selectionId = selection.id;
      box.dataset.index = String(index + 1);
      box.style.cssText = `left:${selection.x}px;top:${selection.y}px;width:${selection.width}px;height:${selection.height}px`;
      for (const handle of ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const) box.insertAdjacentHTML('beforeend', `<button class="hs-dev-review__handle" type="button" data-handle="${handle}" aria-label="Resize capture ${index + 1} from ${handle}"></button>`);
      root.append(box);
    });
  };

  const capture = async (selection: Selection) => {
    const bounded = clampRectToViewport(selection, view.innerWidth, view.innerHeight);
    const canvas = await html2canvas(doc.documentElement, {
      x: bounded.x + view.scrollX, y: bounded.y + view.scrollY, width: bounded.width, height: bounded.height,
      scrollX: -view.scrollX, scrollY: -view.scrollY, backgroundColor: null, logging: false,
      ignoreElements: element => element.hasAttribute('data-hotsheet-dev-review'),
    });
    selection.capture = { id: selection.id, filename: `ux-feedback-${selection.id}.png`, dataUrl: canvas.toDataURL('image/png'), width: canvas.width, height: canvas.height };
  };

  const scheduleCapture = (selection: Selection) => {
    if (selection.captureTimer) view.clearTimeout(selection.captureTimer);
    selection.captureTimer = view.setTimeout(() => { selection.captureTimer = undefined; void capture(selection); }, debounceMs);
  };

  const flushCaptures = async () => {
    await Promise.all(selections.map(async selection => {
      if (selection.captureTimer) { view.clearTimeout(selection.captureTimer); selection.captureTimer = undefined; }
      if (!selection.capture) await capture(selection);
    }));
  };

  const openDialog = async () => {
    await flushCaptures();
    selectedPreview = 0;
    const dialog = doc.createElement('dialog');
    dialog.className = 'hs-dev-review__dialog';
    dialog.dataset.hotsheetDevReview = 'true';
    dialog.setAttribute('aria-labelledby', 'hs-dev-review-dialog-title');
    const captures = selections.flatMap(selection => selection.capture ? [selection.capture] : []);
    dialog.innerHTML = `<form method="dialog" class="hs-dev-review__form"><header><h2 id="hs-dev-review-dialog-title">New Hot Sheet ticket</h2><button type="button" data-action="close-dialog">Cancel</button></header><div class="hs-dev-review__dialog-body"><div class="hs-dev-review__thumbnails" aria-label="Captured regions"></div><div class="hs-dev-review__preview" aria-label="Selected capture preview"></div><label>Feedback notes<textarea name="notes" required placeholder="Describe the issue or requested change…"></textarea></label><p class="hs-dev-review__status" role="status"></p></div><footer><button type="submit">Create Ticket</button></footer></form>`;
    doc.body.append(dialog);
    const thumbnails = dialog.querySelector<HTMLElement>('.hs-dev-review__thumbnails')!;
    const preview = dialog.querySelector<HTMLElement>('.hs-dev-review__preview')!;
    const showPreview = (index: number) => {
      selectedPreview = index;
      thumbnails.querySelectorAll('button').forEach((button, buttonIndex) => button.setAttribute('aria-pressed', String(buttonIndex === index)));
      preview.innerHTML = captures[index] ? `<img src="${captures[index].dataUrl}" alt="Captured region ${index + 1} preview">` : '<span>No captured regions</span>';
    };
    captures.forEach((item, index) => {
      const button = doc.createElement('button');
      button.type = 'button'; button.className = 'hs-dev-review__thumbnail'; button.setAttribute('aria-label', `Review captured region ${index + 1}`);
      button.innerHTML = `<img src="${item.dataUrl}" alt="">`;
      button.addEventListener('click', () => showPreview(index));
      thumbnails.append(button);
    });
    showPreview(selectedPreview);
    dialog.querySelectorAll<HTMLElement>('[data-action="close-dialog"]').forEach(button => button.addEventListener('click', () => dialog.close()));
    dialog.addEventListener('close', () => dialog.remove());
    dialog.querySelector('form')!.addEventListener('submit', async event => {
      event.preventDefault();
      if (submitting) return;
      const textarea = dialog.querySelector<HTMLTextAreaElement>('textarea')!;
      if (!textarea.reportValidity()) return;
      submitting = true;
      const submit = dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!;
      const status = dialog.querySelector<HTMLElement>('.hs-dev-review__status')!;
      submit.disabled = true; status.textContent = 'Creating ticket and attaching captures…';
      try {
        const result = await options.submit({ notes: textarea.value.trim(), captures, pageUrl: view.location.href, viewport: { width: view.innerWidth, height: view.innerHeight } });
        status.textContent = `${result.slug} created.`;
        leaveFeedback();
        view.setTimeout(() => dialog.close(), 500);
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : 'Ticket creation failed.';
        submit.disabled = false;
      } finally { submitting = false; }
    });
    dialog.showModal();
    dialog.querySelector<HTMLTextAreaElement>('textarea')!.focus();
  };

  const onRootClick = (event: Event) => {
    const target = event.target as HTMLElement;
    if (target.closest('.hs-dev-review__feedback')) {
      if (enabled) {
        if (selections.length > 0 && !view.confirm('Discard the captured feedback regions?')) return;
        leaveFeedback();
      } else { enabled = true; hintVisible = true; render(); scheduleHintFade(); }
      return;
    }
    if (target.closest('[data-action="new-ticket"]')) void openDialog();
  };
  root.addEventListener('click', onRootClick);

  const onPointerDown = (event: PointerEvent) => {
    if (!enabled || event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (event.altKey && !target.closest('.hs-dev-review__toolbar,.hs-dev-review__dialog')) {
      event.preventDefault(); event.stopPropagation();
      const id = String(sequence++);
      selections.push({ id, x: event.clientX, y: event.clientY, width: 1, height: 1 });
      gesture = { kind: 'draw', id, startX: event.clientX, startY: event.clientY };
      render();
      return;
    }
    const box = target.closest<HTMLElement>('.hs-dev-review__rect');
    if (box) {
      event.preventDefault(); event.stopPropagation();
      const selection = selections.find(item => item.id === box.dataset.selectionId)!;
      if (selection.captureTimer) { view.clearTimeout(selection.captureTimer); selection.captureTimer = undefined; }
      const handle = target.closest<HTMLElement>('[data-handle]')?.dataset.handle as ResizeHandle | undefined;
      gesture = handle ? { kind: 'resize', id: selection.id, handle } : { kind: 'move', id: selection.id, startX: event.clientX, startY: event.clientY, origin: { ...selection } };
      return;
    }
  };

  const onPointerMove = (event: PointerEvent) => {
    if (!gesture) return;
    event.preventDefault();
    const index = selections.findIndex(item => item.id === gesture!.id);
    if (index < 0) return;
    const current = selections[index];
    const next = gesture.kind === 'draw' ? normalizeRect(current.id, gesture.startX, gesture.startY, event.clientX, event.clientY)
      : gesture.kind === 'resize' ? resizeRect(current, gesture.handle, event.clientX, event.clientY)
        : { ...gesture.origin, x: gesture.origin.x + event.clientX - gesture.startX, y: gesture.origin.y + event.clientY - gesture.startY };
    selections[index] = { ...clampRectToViewport(next, view.innerWidth, view.innerHeight), capture: undefined };
    render();
  };

  const onPointerUp = () => {
    if (!gesture) return;
    const selection = selections.find(item => item.id === gesture!.id);
    gesture = undefined;
    if (!selection) return;
    if (selection.width < 12 || selection.height < 12) selections.splice(selections.indexOf(selection), 1);
    else scheduleCapture(selection);
    render();
  };
  doc.addEventListener('pointerdown', onPointerDown, true);
  const onKeyChange = (event: KeyboardEvent) => setModifierHeld(event.altKey);
  const onWindowBlur = () => setModifierHeld(false);
  doc.addEventListener('keydown', onKeyChange, true);
  doc.addEventListener('keyup', onKeyChange, true);
  view.addEventListener('blur', onWindowBlur);
  view.addEventListener('pointermove', onPointerMove, true);
  view.addEventListener('pointerup', onPointerUp, true);
  render();

  return { destroy() { if (hintTimer) view.clearTimeout(hintTimer); setModifierHeld(false); root.removeEventListener('click', onRootClick); doc.removeEventListener('pointerdown', onPointerDown, true); doc.removeEventListener('keydown', onKeyChange, true); doc.removeEventListener('keyup', onKeyChange, true); view.removeEventListener('blur', onWindowBlur); view.removeEventListener('pointermove', onPointerMove, true); view.removeEventListener('pointerup', onPointerUp, true); root.remove(); } };
}
