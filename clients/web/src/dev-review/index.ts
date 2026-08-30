import './dev-review.css';

import html2canvas from 'html2canvas';

import { clampRectToViewport, intersectRectWithViewport, normalizeRect, type ResizeHandle, resizeRect, type ReviewRect,translateAnchoredRect } from './geometry';

export interface ReviewCapture { id: string; filename: string; dataUrl: string; width: number; height: number }
export interface ReviewAttachment { id: string; filename: string; dataUrl: string; mimeType: string; size: number }
export interface DevReviewSubmission { notes: string; captures: ReviewCapture[]; attachments: ReviewAttachment[]; pageUrl: string; viewport: { width: number; height: number } }
export interface DevReviewResult { slug: string; url?: string }
export interface DevReviewOptions { submit: (submission: DevReviewSubmission) => Promise<DevReviewResult>; captureDebounceMs?: number; hintDurationMs?: number; document?: Document }
interface Selection extends ReviewRect { capture?: ReviewCapture; captureTimer?: number; capturePromise?: Promise<void>; revision: number; anchor?: { element: Element; point: { x: number; y: number } } }
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
  let deleteModifierHeld = false;
  let scrollFrame: number | undefined;

  const root = doc.createElement('div');
  root.className = 'hs-dev-review';
  root.dataset.hotsheetDevReview = 'true';
  root.innerHTML = '<div class="hs-dev-review__toolbar"><button class="hs-dev-review__feedback" type="button" aria-pressed="false">Feedback</button></div>';
  doc.body.append(root);
  const toolbar = root.querySelector<HTMLElement>('.hs-dev-review__toolbar')!;

  const setModifiers = (alt: boolean, shift = false) => {
    deleteModifierHeld = enabled && alt && shift;
    modifierHeld = enabled && alt && !shift;
    doc.documentElement.classList.toggle('hs-dev-review--crosshair', modifierHeld);
    doc.documentElement.classList.toggle('hs-dev-review--delete', deleteModifierHeld);
    root.dataset.modifierHeld = String(modifierHeld);
    root.dataset.deleteModifierHeld = String(deleteModifierHeld);
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
    setModifiers(false);
    enabled = false;
    selections.splice(0);
    render();
  };

  const render = () => {
    toolbar.innerHTML = enabled
      ? '<button class="hs-dev-review__feedback" type="button" aria-pressed="true">Feedback</button><button data-action="new-ticket" type="button">New Ticket</button>'
      : '<button class="hs-dev-review__feedback" type="button" aria-pressed="false">Feedback</button>';
    root.querySelectorAll('.hs-dev-review__hint,.hs-dev-review__rect').forEach(node => { node.remove(); });
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

  const updateSelectionElement = (selection: Selection) => {
    const box = root.querySelector<HTMLElement>(`.hs-dev-review__rect[data-selection-id="${selection.id}"]`);
    if (box) box.style.cssText = `left:${selection.x}px;top:${selection.y}px;width:${selection.width}px;height:${selection.height}px`;
  };

  const anchorSelection = (selection: Selection) => {
    const element = doc.elementsFromPoint(selection.x + 1, selection.y + 1).find(candidate => !root.contains(candidate));
    if (!element) { selection.anchor = undefined; return; }
    const bounds = element.getBoundingClientRect();
    selection.anchor = { element, point: { x: bounds.left, y: bounds.top } };
  };

  const updateAnchoredSelections = () => {
    scrollFrame = undefined;
    for (const selection of selections) {
      if (!selection.anchor?.element.isConnected || gesture?.id === selection.id) continue;
      const bounds = selection.anchor.element.getBoundingClientRect();
      const next = translateAnchoredRect(selection, selection.anchor.point, { x: bounds.left, y: bounds.top });
      selection.x = next.x; selection.y = next.y;
      selection.anchor.point = { x: bounds.left, y: bounds.top };
      updateSelectionElement(selection);
    }
  };

  const captureSelections = (targets: Selection[]): Promise<void> => {
    const revisions = new Map(targets.map(selection => [selection.id, selection.revision]));
    const promise = (async () => {
      for (const selection of targets) {
        if (selection.revision !== revisions.get(selection.id)) continue;
        const bounded = intersectRectWithViewport(selection, view.innerWidth, view.innerHeight);
        const cropped = await html2canvas(doc.documentElement, {
          x: bounded.x + view.scrollX, y: bounded.y + view.scrollY, width: bounded.width, height: bounded.height,
          windowWidth: view.innerWidth, windowHeight: view.innerHeight, scrollX: view.scrollX, scrollY: view.scrollY,
          // Review rectangles use CSS viewport pixels; keep the output in that same
          // coordinate space on Retina/high-DPI displays.
          scale: 1, backgroundColor: null, logging: false,
          ignoreElements: element => element.hasAttribute('data-hotsheet-dev-review'),
        });
        selection.capture = { id: selection.id, filename: `ux-feedback-${selection.id}.png`, dataUrl: cropped.toDataURL('image/png'), width: cropped.width, height: cropped.height };
      }
    })().finally(() => { for (const selection of targets) if (selection.capturePromise === promise) selection.capturePromise = undefined; });
    for (const selection of targets) selection.capturePromise = promise;
    return promise;
  };

  const scheduleCapture = (selection: Selection) => {
    if (selection.captureTimer) view.clearTimeout(selection.captureTimer);
    selection.captureTimer = view.setTimeout(() => { selection.captureTimer = undefined; void captureSelections([selection]); }, debounceMs);
  };

  const flushCaptures = async () => {
    for (const selection of selections) if (selection.captureTimer) { view.clearTimeout(selection.captureTimer); selection.captureTimer = undefined; }
    await Promise.all([...new Set(selections.flatMap(selection => selection.capturePromise ? [selection.capturePromise] : []))]);
    const missing = selections.filter(selection => !selection.capture);
    if (missing.length > 0) await captureSelections(missing);
  };

  const openDialog = async () => {
    selectedPreview = 0;
    const dialog = doc.createElement('dialog');
    dialog.className = 'hs-dev-review__dialog';
    dialog.dataset.hotsheetDevReview = 'true';
    dialog.setAttribute('aria-labelledby', 'hs-dev-review-dialog-title');
    let captures: ReviewCapture[] = [];
    const attachments: ReviewAttachment[] = [];
    dialog.innerHTML = `<form method="dialog" class="hs-dev-review__form"><header><h2 id="hs-dev-review-dialog-title">New Hot Sheet ticket</h2><button type="button" data-action="close-dialog">Cancel</button></header><div class="hs-dev-review__dialog-body"><div class="hs-dev-review__thumbnails" aria-label="Captured regions"></div><div class="hs-dev-review__preview" aria-label="Selected capture preview"></div><label class="hs-dev-review__dropzone">Drop attachments here or <span>browse</span><input type="file" multiple aria-label="Add attachments"></label><div class="hs-dev-review__attachments" aria-label="Added attachments"></div><label>Feedback notes<textarea name="notes" required placeholder="Describe the issue or requested change…"></textarea></label><p class="hs-dev-review__status" role="status"></p></div><footer><button type="submit">Create Ticket</button></footer></form>`;
    doc.body.append(dialog);
    const thumbnails = dialog.querySelector<HTMLElement>('.hs-dev-review__thumbnails')!;
    const preview = dialog.querySelector<HTMLElement>('.hs-dev-review__preview')!;
    const status = dialog.querySelector<HTMLElement>('.hs-dev-review__status')!;
    const submit = dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    const dropzone = dialog.querySelector<HTMLElement>('.hs-dev-review__dropzone')!;
    const input = dialog.querySelector<HTMLInputElement>('input[type="file"]')!;
    const attachmentList = dialog.querySelector<HTMLElement>('.hs-dev-review__attachments')!;
    const showPreview = (index: number) => {
      selectedPreview = index;
      thumbnails.querySelectorAll<HTMLButtonElement>('[data-action="review-capture"]').forEach((button, buttonIndex) => { button.setAttribute('aria-pressed', String(buttonIndex === index)); });
      preview.innerHTML = captures[index] ? `<img src="${captures[index].dataUrl}" alt="Captured region ${index + 1} preview">` : '<span>No captured regions</span>';
    };
    const showCaptures = () => {
      thumbnails.replaceChildren();
      captures.forEach((item, index) => {
        const wrapper = doc.createElement('div'); wrapper.className = 'hs-dev-review__item';
        const button = doc.createElement('button'); button.type = 'button'; button.className = 'hs-dev-review__thumbnail'; button.dataset.action = 'review-capture'; button.setAttribute('aria-label', `Review captured region ${index + 1}`); button.innerHTML = `<img src="${item.dataUrl}" alt="">`; button.addEventListener('click', () => { showPreview(index); });
        const remove = doc.createElement('button'); remove.type = 'button'; remove.className = 'hs-dev-review__remove'; remove.setAttribute('aria-label', `Remove captured region ${index + 1}`); remove.addEventListener('click', () => { captures.splice(index, 1); const selectionIndex = selections.findIndex(selection => selection.id === item.id); if (selectionIndex >= 0) selections.splice(selectionIndex, 1); selectedPreview = Math.min(selectedPreview, Math.max(0, captures.length - 1)); render(); showCaptures(); });
        wrapper.append(button, remove); thumbnails.append(wrapper);
      });
      showPreview(selectedPreview);
    };
    const showAttachments = () => {
      attachmentList.replaceChildren();
      attachments.forEach((item, index) => {
        const wrapper = doc.createElement('div'); wrapper.className = 'hs-dev-review__attachment';
        if (item.mimeType.startsWith('image/')) { const image = doc.createElement('img'); image.src = item.dataUrl; image.alt = ''; wrapper.append(image); }
        const name = doc.createElement('span'); name.textContent = item.filename;
        const remove = doc.createElement('button'); remove.type = 'button'; remove.className = 'hs-dev-review__remove'; remove.setAttribute('aria-label', `Remove attachment ${item.filename}`); remove.addEventListener('click', () => { attachments.splice(index, 1); showAttachments(); });
        wrapper.append(name, remove); attachmentList.append(wrapper);
      });
    };
    const addFiles = async (files: FileList | File[]) => {
      for (const file of Array.from(files)) {
        const dataUrl = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => { resolve(String(reader.result)); }; reader.onerror = () => { reject(reader.error); }; reader.readAsDataURL(file); });
        attachments.push({ id: `attachment-${Date.now()}-${attachments.length}`, filename: file.name, dataUrl, mimeType: file.type || 'application/octet-stream', size: file.size });
      }
      showAttachments();
    };
    input.addEventListener('change', () => { if (input.files) void addFiles(input.files); input.value = ''; });
    for (const type of ['dragenter', 'dragover']) dropzone.addEventListener(type, event => { event.preventDefault(); dropzone.dataset.dragging = 'true'; });
    for (const type of ['dragleave', 'drop']) dropzone.addEventListener(type, event => { event.preventDefault(); delete dropzone.dataset.dragging; });
    dropzone.addEventListener('drop', event => { if (event.dataTransfer?.files.length) void addFiles(event.dataTransfer.files); });
    dialog.querySelectorAll<HTMLElement>('[data-action="close-dialog"]').forEach(button => { button.addEventListener('click', () => { dialog.close(); }); });
    dialog.addEventListener('close', () => { dialog.remove(); });
    dialog.querySelector('form')!.addEventListener('submit', async event => {
      event.preventDefault();
      if (submitting) return;
      const textarea = dialog.querySelector<HTMLTextAreaElement>('textarea')!;
      if (!textarea.reportValidity()) return;
      submitting = true;
      submit.disabled = true; status.textContent = 'Creating ticket and attaching captures…';
      try {
        const result = await options.submit({ notes: textarea.value.trim(), captures, attachments, pageUrl: view.location.href, viewport: { width: view.innerWidth, height: view.innerHeight } });
        status.textContent = `${result.slug} created.`;
        leaveFeedback();
        view.setTimeout(() => { dialog.close(); }, 500);
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : 'Ticket creation failed.';
        submit.disabled = false;
      } finally { submitting = false; }
    });
    dialog.showModal();
    dialog.querySelector<HTMLTextAreaElement>('textarea')!.focus();
    submit.disabled = true; status.textContent = 'Preparing captures…'; preview.innerHTML = '<span>Preparing captures…</span>';
    try { await flushCaptures(); captures = selections.flatMap(selection => selection.capture ? [selection.capture] : []); showCaptures(); status.textContent = ''; submit.disabled = false; }
    catch (error) { status.textContent = error instanceof Error ? error.message : 'Capture failed.'; }
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
    const box = target.closest<HTMLElement>('.hs-dev-review__rect');
    if (box && event.altKey && event.shiftKey) {
      event.preventDefault(); event.stopPropagation();
      const index = selections.findIndex(item => item.id === box.dataset.selectionId);
      if (index >= 0) {
        const [removed] = selections.splice(index, 1);
        if (removed.captureTimer) view.clearTimeout(removed.captureTimer);
        render();
      }
      return;
    }
    if (event.altKey && !target.closest('.hs-dev-review__toolbar,.hs-dev-review__dialog')) {
      event.preventDefault(); event.stopPropagation();
      const id = String(sequence++);
      selections.push({ id, x: event.clientX, y: event.clientY, width: 1, height: 1, revision: 0 });
      gesture = { kind: 'draw', id, startX: event.clientX, startY: event.clientY };
      render();
      return;
    }
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
    selections[index] = { ...current, ...clampRectToViewport(next, view.innerWidth, view.innerHeight), capture: undefined, revision: current.revision + 1 };
    updateSelectionElement(selections[index]);
  };

  const onPointerUp = () => {
    if (!gesture) return;
    const selection = selections.find(item => item.id === gesture!.id);
    gesture = undefined;
    if (!selection) return;
    if (selection.width < 12 || selection.height < 12) { selections.splice(selections.indexOf(selection), 1); render(); }
    else { anchorSelection(selection); scheduleCapture(selection); updateSelectionElement(selection); }
  };
  doc.addEventListener('pointerdown', onPointerDown, true);
  const onKeyChange = (event: KeyboardEvent) => { setModifiers(event.altKey, event.shiftKey); };
  const onWindowBlur = () => { setModifiers(false); };
  doc.addEventListener('keydown', onKeyChange, true);
  doc.addEventListener('keyup', onKeyChange, true);
  view.addEventListener('blur', onWindowBlur);
  view.addEventListener('pointermove', onPointerMove, true);
  view.addEventListener('pointerup', onPointerUp, true);
  const onScroll = () => { if (scrollFrame === undefined) scrollFrame = view.requestAnimationFrame(updateAnchoredSelections); };
  doc.addEventListener('scroll', onScroll, true);
  view.addEventListener('resize', onScroll);
  render();

  return { destroy() { if (hintTimer) view.clearTimeout(hintTimer); if (scrollFrame !== undefined) view.cancelAnimationFrame(scrollFrame); setModifiers(false); root.removeEventListener('click', onRootClick); doc.removeEventListener('pointerdown', onPointerDown, true); doc.removeEventListener('keydown', onKeyChange, true); doc.removeEventListener('keyup', onKeyChange, true); doc.removeEventListener('scroll', onScroll, true); view.removeEventListener('resize', onScroll); view.removeEventListener('blur', onWindowBlur); view.removeEventListener('pointermove', onPointerMove, true); view.removeEventListener('pointerup', onPointerUp, true); root.remove(); } };
}
