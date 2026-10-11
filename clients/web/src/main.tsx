import '@kerfjs/ui/select/register';
import '@kerfjs/ui/popup-menu/register';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@kerfjs/ui/webawesome.css';
import './components/heading.css';
import './hot-sheet-tokens.css';
import './style.css';

import { wireResizableRegions } from '@kerfjs/ui/wire-resizable-regions';
import { wireScrollDividers } from '@kerfjs/ui/wire-scroll-dividers';

import { startHotSheetWebClient } from './app/runtime';
import { installDocumentScrollRestore } from './document-scroll-restore';

const { appRoot } = await startHotSheetWebClient();
// iOS keyboard scrolls of the clipped app root are not user-recoverable; undo them on dismissal (HS2-BCA512).
installDocumentScrollRestore(window);
// Kerf panes, NavStacks, TabScaffolds, and TabBar strips draw their chrome dividers only while content
// scrolls beneath them; one page-lifetime instance at the application root (HS2-TAZJ0V, HS2-TF76Z2).
void wireScrollDividers(appRoot);
// SplitView's documented wiring contract (Repository Status list-detail, HS2-3B8345). Its split is not
// resizable today, so there is no size to persist; a future resizable region commits through here.
void wireResizableRegions(appRoot, { onCommit: () => undefined });
