import '@kerfjs/ui/select/register';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@kerfjs/ui/webawesome.css';
import './components/heading.css';
import './hot-sheet-tokens.css';
import './style.css';

import { startHotSheetWebClient } from './app/runtime';

await startHotSheetWebClient();
