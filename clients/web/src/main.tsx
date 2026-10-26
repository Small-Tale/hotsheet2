import '@kerfjs/ui/select/register';
import '@kerfjs/ui/popup-menu/register';
// DialogSurface close requests and chrome measurement, from the browser entry only: the register
// touches the document as it loads (Kerf UI 5.0.0-beta.106), which node-environment unit tests lack.
import '@kerfjs/ui/surface-scaffold/register';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
// Kerf's foundation owns the canonical --kui-space-* scale and typography tokens (HS2-0HT772).
import '@kerfjs/ui/foundation.css';
import '@kerfjs/ui/webawesome.css';
import './hot-sheet-tokens.css';
import './style.css';

import { wireChoiceChips } from '@kerfjs/ui/wire-choice-chips';
import { wireCornerStatus } from '@kerfjs/ui/wire-corner-status';
import { wireResizableRegions } from '@kerfjs/ui/wire-resizable-regions';
import { wireScrollDividers } from '@kerfjs/ui/wire-scroll-dividers';
import { wireTextFields } from '@kerfjs/ui/wire-text-fields';

import { startHotSheetWebClient } from './app/runtime';
import { TERMINAL_VISIBILITY_SCROLL_TARGET } from './components/terminal-visibility-dialog';
import { initializeDesktopStorage } from './desktop-storage';
import { installDocumentScrollRestore } from './document-scroll-restore';

// The release shell marks this window; ordinary browser and Vite sessions keep their own storage.
if ('__HOTSHEET_DESKTOP_STORAGE__' in window) await initializeDesktopStorage();

const { appRoot } = await startHotSheetWebClient();
// iOS keyboard scrolls of the clipped app root are not user-recoverable; undo them on dismissal (HS2-BCA512).
installDocumentScrollRestore(window);
// Kerf panes, NavStacks, TabScaffolds, and TabBar strips draw their chrome dividers only while content
// scrolls beneath them; one page-lifetime instance at the application root (HS2-TAZJ0V, HS2-TF76Z2).
void wireScrollDividers(appRoot, { targets: [TERMINAL_VISIBILITY_SCROLL_TARGET] });
// Opens the corner loading status in the top layer whenever it renders (KF-PZCPHQ, HS2-5C4Y5Q).
void wireCornerStatus(appRoot);
// SplitView's documented wiring contract (Repository Status list-detail, HS2-3B8345). Its split is not
// resizable today, so there is no size to persist; a future resizable region commits through here.
void wireResizableRegions(appRoot, { onCommit: () => undefined });
// ChoiceChips' documented wiring (Kerf beta.106): a `multiple: true` group's listbox selection. The
// app's multi-choice groups are `multiple: 'toggle'` checkboxes today, which need none (HS2-BH7NZ4).
void wireChoiceChips(appRoot);
// The clearable permission-rules search emits native input/change after Clear (HS2-CQ1TZN).
void wireTextFields(appRoot);
