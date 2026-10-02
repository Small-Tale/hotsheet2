import '@kerfjs/ui/select/register';
import '@kerfjs/ui/popup-menu/register';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@kerfjs/ui/webawesome.css';
import './components/heading.css';
import './hot-sheet-tokens.css';
import './style.css';

import { wireScrollDividers } from '@kerfjs/ui/wire-scroll-dividers';

import { startHotSheetWebClient } from './app/runtime';

const { appRoot } = await startHotSheetWebClient();
// Kerf panes, NavStacks, TabScaffolds, and TabBar strips draw their chrome dividers only while content
// scrolls beneath them; one page-lifetime instance at the application root (HS2-TAZJ0V, HS2-TF76Z2).
void wireScrollDividers(appRoot);
