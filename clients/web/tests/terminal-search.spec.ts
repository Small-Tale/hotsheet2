import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { installTerminalFixture } from './terminal-feedback-fixture';

// HS2-TWZSZF: a borderless, iconic search button next to the drawer's hide button expands into a field
// when activated and searches the selected terminal.
test.use({ viewport: { width: 1280, height: 800 } });

async function openDrawerTerminal(page: Page) {
  await installTerminalFixture(page);
  await page.goto('/');
  await openMockedProject(page);
  await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
  await expect(page.locator('.app-shell')).toHaveAttribute('data-terminal-drawer-transitioning', 'false');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
  await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
  const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  return { drawer, viewport };
}

test('HS2-TWZSZF terminal search expands from a borderless icon, selects matches, and collapses again', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    // The active match is the terminal's selection, read through the viewport's clipboard event.
    selected = () =>
      viewport.evaluate((node) => {
        const detail: { selectionOnly: boolean; text?: string } = { selectionOnly: true };
        node.dispatchEvent(new CustomEvent('hotsheet-terminal-read-text', { detail }));
        return detail.text ?? '';
      });
  // Collapsed: one iconic, borderless control immediately before the Clear and Hide actions.
  await expect(group).toHaveAttribute('data-appearance', 'borderless');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(trigger.locator('[data-lucide="search"]')).toBeVisible();
  // Borderless until activated: no visible border or fill around the icon.
  await expect(group).toHaveCSS('border-top-color', 'rgba(0, 0, 0, 0)');
  await expect(group).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const triggerBox = (await trigger.boundingBox())!,
    clearBox = (await drawer.getByRole('button', { name: 'Clear Terminal' }).boundingBox())!;
  expect(triggerBox.x + triggerBox.width).toBeLessThanOrEqual(clearBox.x);
  await page.screenshot({ path: test.info().outputPath('hs2-twzszf-collapsed-1280.png') });

  // Activated: Kerf expands the field and focuses its editor.
  await trigger.click();
  await expect(group).toHaveAttribute('data-expanded', 'true');
  await expect(group).toHaveAttribute('data-appearance', 'contained');
  await expect(editor).toBeFocused();

  // Typing searches incrementally and selects the match; Enter and Shift-Enter step without leaving the field.
  await page.keyboard.type('Justify');
  await expect.poll(selected).toBe('Justify');
  await page.keyboard.press('Enter');
  await expect.poll(selected).toBe('Justify');
  await page.keyboard.press('Shift+Enter');
  await expect(editor).toBeFocused();
  await expect(editor).toHaveText('Justify');
  await expect.poll(selected).toBe('Justify');
  await page.screenshot({ path: test.info().outputPath('hs2-twzszf-match-1280.png') });

  // A query with no match selects nothing; a new query finds its match; emptying the field clears it.
  await page.keyboard.type('-absent');
  await expect.poll(selected).toBe('');
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Backspace');
  await page.keyboard.type('nano');
  await expect.poll(selected).toBe('nano');
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Backspace');
  await expect.poll(selected).toBe('');

  // Escape on the empty field collapses it back to the iconic button and returns focus there.
  await page.keyboard.press('Escape');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(group).toHaveAttribute('data-appearance', 'borderless');
  await expect(trigger).toBeFocused();

  // The grid view has no single terminal to search.
  await drawer.getByRole('button', { name: 'Project Grid' }).click();
  await expect(drawer.locator('.kui-toolbar-control-group[data-content="search"]')).toHaveCount(0);
});

test('HS2-SW2YAZ double-clicking in the terminal search field never maximizes the drawer', async ({ page }) => {
  const { drawer } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await trigger.click();
  await expect(group).toHaveAttribute('data-expanded', 'true');
  // The reported gesture: double-click in the field, empty and then on a word, then the group's own edge.
  await editor.dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await page.keyboard.type('Justify');
  await editor.dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(editor).toBeFocused();
  const box = (await group.boundingBox())!;
  await page.mouse.dblclick(box.x + 2, box.y + box.height / 2);
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  // The rail's empty space still maximizes and restores.
  const doubleClickRail = async () => {
    const rail = (await drawer.locator('.terminal-drawer__rail').boundingBox())!;
    await page.mouse.dblclick(rail.x + rail.width / 2, rail.y + rail.height - 2);
  };
  await doubleClickRail();
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await doubleClickRail();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
});

type SearchSocketWindow = typeof window & {
  __terminalFeedbackSockets: Array<EventTarget & { url: string; readyState: number }>;
};

/** Write output to a terminal through its newest open fake socket, as the PTY would. */
async function writeTerminalOutput(page: Page, terminalId: string, output: string) {
  await page.evaluate(
    ({ terminalId, output }) => {
      const socket = (window as SearchSocketWindow).__terminalFeedbackSockets
        .filter((item) => item.url.includes(`/terminals/${terminalId}/`) && item.readyState === 1)
        .at(-1)!;
      socket.dispatchEvent(new MessageEvent('message', { data: new TextEncoder().encode(output).buffer }));
    },
    { terminalId, output },
  );
}

test('HS2-M5QPED terminal search highlights every match and counts them through edits, output, clearing, closing, and terminal switches', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    count = drawer.locator('[data-terminal-search-count]'),
    // xterm draws one decoration per match plus one more over the active match (addon-search 0.16 marks
    // neither with its active class), so N matches render N + 1 decorations.
    highlights = (target = viewport) => target.locator('.xterm-find-result-decoration'),
    replaceQuery = async (query: string) => {
      await editor.click();
      await page.keyboard.press('ControlOrMeta+a');
      await page.keyboard.press('Backspace');
      if (query) await page.keyboard.type(query);
    };
  await trigger.click();
  await expect(editor).toBeFocused();
  // An open, empty field shows no count.
  await expect(count).toHaveCount(0);

  // One match: highlighted as the active match, counted beside the field.
  await page.keyboard.type('Justify');
  await expect(count).toHaveText('1 of 1');
  await expect(highlights()).toHaveCount(2);
  // The count sits after the search group and before the Clear and Hide actions.
  const searchBox = (await group.boundingBox())!,
    countBox = (await count.boundingBox())!,
    clearBox = (await drawer.getByRole('button', { name: 'Clear Terminal', exact: true }).boundingBox())!;
  expect(countBox.width).toBeGreaterThan(0);
  expect(countBox.x).toBeGreaterThanOrEqual(searchBox.x + searchBox.width - 1);
  expect(countBox.x + countBox.width).toBeLessThanOrEqual(clearBox.x + 1);

  // Query edits re-count: several matches, stepped with Enter and Shift-Enter.
  await replaceQuery('File');
  await expect(count).toHaveText(/^1 of \d+$/);
  const total = Number((await count.textContent())!.split(' of ')[1]);
  expect(total).toBeGreaterThanOrEqual(2);
  await expect(highlights()).toHaveCount(total + 1);
  await page.keyboard.press('Enter');
  await expect(count).toHaveText(`2 of ${total}`);
  await page.keyboard.press('Shift+Enter');
  await expect(count).toHaveText(`1 of ${total}`);
  await expect(highlights()).toHaveCount(total + 1);
  // The count is a control too: a click steps to the next match and keeps typing in the field.
  await count.click();
  await expect(count).toHaveText(`2 of ${total}`);
  await expect(editor).toBeFocused();
  await count.click({ modifiers: ['Shift'] });
  await expect(count).toHaveText(`1 of ${total}`);

  // No match: the count says so and nothing is highlighted.
  await page.keyboard.type('-absent');
  await expect(count).toHaveText('No matches');
  await expect(count).toBeDisabled();
  await expect(highlights()).toHaveCount(0);

  // Output arriving while the search is open re-counts the matches.
  await replaceQuery('Justify');
  await expect(count).toHaveText('1 of 1');
  await writeTerminalOutput(page, 'terminal-new', '\u001b[6;1HJustify once more, Justify twice');
  // HS2-QHFP4A: the count is re-searched from the rendered buffer, so wait for the output to land
  // there first; a run that never delivers it now fails here instead of on a stale "1 of 1".
  await expect(viewport.locator('.xterm-rows')).toContainText('Justify once more, Justify twice');
  await expect(count).toHaveText(/^\d of 3$/);
  await expect(highlights()).toHaveCount(4);
  await page.screenshot({ path: test.info().outputPath('hs2-m5qped-matches-1280.png') });

  // Clearing the field removes the highlights and the count; refilling brings them back.
  await group.getByRole('button', { name: 'Clear Terminal Search' }).click();
  await expect(editor).toBeFocused();
  await expect(count).toHaveCount(0);
  await expect(highlights()).toHaveCount(0);
  await page.keyboard.type('Justify');
  await expect(count).toHaveText(/^\d of 3$/);

  // Switching terminals moves the search: the left terminal's highlights go, the new one is counted.
  await drawer.locator('[data-tab-id="nano"] [data-action="select-drawer-item"]').click();
  const nano = drawer.locator('[data-component="terminal-session"]:not([hidden]) [data-terminal-id="nano"]');
  await expect(nano).toHaveAttribute('data-connection', 'connected');
  await expect(count).toHaveText('1 of 1');
  await expect(highlights(nano)).toHaveCount(2);
  await expect(highlights(viewport)).toHaveCount(0);
  await drawer.locator('[data-tab-id="terminal-new"] [data-action="select-drawer-item"]').click();
  // Back on the first terminal (its fake PTY redrew the nano screen on resume, so one match again), the
  // search follows and the second terminal's highlights go.
  await expect(highlights()).toHaveCount(2);
  await expect(count).toHaveText('1 of 1');
  await expect(highlights(nano)).toHaveCount(0);

  // Closing (empty the field, then Escape) removes the highlights and the count.
  await replaceQuery('');
  await page.keyboard.press('Escape');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(count).toHaveCount(0);
  await expect(highlights()).toHaveCount(0);
});

test('HS2-HPDX81 the open search, its count, and Hide stay inside a phone-width drawer through wide-narrow-wide resizes', async ({
  page,
}) => {
  const { drawer } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    count = drawer.locator('[data-terminal-search-count]'),
    status = drawer.locator('[data-terminal-search-count-status]'),
    hide = drawer.getByRole('button', { name: 'Hide Terminal Drawer' }),
    clear = drawer.getByRole('button', { name: 'Clear Terminal', exact: true });
  // Every listed rail control lies inside the drawer's own box (no clipping past its right edge), once
  // the shell's resize transitions have settled.
  const expectInsideDrawer = async (...controls: ReturnType<typeof page.locator>[]) => {
    await page.evaluate(() =>
      Promise.all(
        document
          .getAnimations()
          .filter((animation) => animation instanceof CSSTransition)
          .map((animation) => animation.finished.catch(() => undefined)),
      ),
    );
    const bounds = (await drawer.boundingBox())!;
    for (const control of controls) {
      const box = (await control.boundingBox())!;
      expect(box.width).toBeGreaterThan(0);
      expect(box.x).toBeGreaterThanOrEqual(bounds.x);
      expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    }
  };

  // Wide: the full count control sits beside the field.
  await group.getByRole('button', { name: 'Search Terminal' }).click();
  await page.keyboard.type('File');
  await expect(count).toHaveText(/^1 of \d+$/);
  const total = Number((await count.textContent())!.split(' of ')[1]);
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);

  // Phone: the rail keeps only a pressed Search button beside Clear and Hide (HS2-7HPXCY); the field,
  // the count, and its status leave the rail, and the kept search goes on highlighting.
  await page.setViewportSize({ width: 390, height: 844 });
  const phoneSearch = drawer.getByRole('button', { name: 'Search Terminal' }),
    bar = page.locator('[data-component="terminal-focus-search"]'),
    barEditor = bar.locator('[data-search-field-editor="terminal-search"]'),
    barStatus = bar.locator('[data-terminal-focus-search-status]');
  await expect(group).toHaveCount(0);
  await expect(count).toHaveCount(0);
  await expect(status).toHaveCount(0);
  await expect(phoneSearch).toHaveAttribute('aria-pressed', 'true');
  await expectInsideDrawer(phoneSearch, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-count-390.png') });
  // No matches, the state that pushed Clear and Hide out of the drawer before the fix: typed in the
  // full-screen search bar, whose count line says so.
  await phoneSearch.click();
  await expect(barEditor).toBeFocused();
  await expect(barStatus).toHaveText(`1 of ${total}`);
  await page.keyboard.type('-absent');
  await expect(barStatus).toHaveText('No matches');
  await bar.getByRole('button', { name: 'Exit terminal focus' }).click();
  await expect(phoneSearch).toBeFocused();
  await expectInsideDrawer(phoneSearch, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-390.png') });

  // Medium: the field returns to the rail with the kept query, and the full count control beside it.
  await page.setViewportSize({ width: 768, height: 900 });
  await expect(editor).toHaveText('File-absent');
  await expect(count).toHaveText('No matches');
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-768.png') });

  // Wide again: the full count control returns and everything still fits.
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(count).toHaveText('No matches');
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-1280.png') });
});

interface SearchReport {
  resultIndex: number;
  resultCount: number;
}

test('HS2-SWY5DQ the active terminal search match survives wide-narrow-wide resizes, stays on screen (HS2-X96PS0), and Enter steps on from it', async ({
  page,
}) => {
  const { drawer } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    // The full count control, its screen-reader status when a narrow rail hides it (HS2-HPDX81), or the
    // phone search bar's status (HS2-7HPXCY).
    count = page.locator(
      '[data-component="terminal-drawer"] :is([data-terminal-search-count], [data-terminal-search-count-status], [data-terminal-focus-search-status])',
    ),
    // The rail's field, or the phone search bar's field while it is open.
    activeEditor = page.locator('[data-search-field-editor="terminal-search"]');
  // Record every results report the viewport publishes, so a resize is judged by what it reported.
  await page.evaluate(() => {
    const reports: SearchReport[] = [];
    (window as typeof window & { __searchReports: SearchReport[] }).__searchReports = reports;
    document.addEventListener('hotsheet-terminal-search-results', (event) => {
      reports.push((event as CustomEvent<SearchReport>).detail);
    });
  });
  const reports = () =>
    page.evaluate(() => (window as typeof window & { __searchReports: SearchReport[] }).__searchReports);
  // The fake PTY redraws nano at each width, so the match total may differ by width; the active index may not.
  const active = (index: number) => expect(count).toHaveText(new RegExp(`^${index} of \\d+$`));
  const total = async () => Number((await count.textContent())!.split(' of ')[1]);
  await group.getByRole('button', { name: 'Search Terminal' }).click();
  await page.keyboard.type('File');
  await active(1);
  expect(await total()).toBeGreaterThanOrEqual(2);

  // Resize, wait for the terminal to re-search its reflowed buffer and the shell to settle, and require
  // that no report since the resize moved the active match.
  const resizeKeeping = async (width: number, height: number, index: number, phone = false) => {
    const before = (await reports()).length;
    await page.setViewportSize({ width, height });
    // A phone rail has no field (HS2-7HPXCY): its Search opens the search bar, which keeps the query.
    if (phone) await drawer.getByRole('button', { name: 'Search Terminal' }).click();
    await expect
      .poll(async () => (await reports()).length, { message: `re-search at ${width}px` })
      .toBeGreaterThan(before);
    await page.evaluate(() =>
      Promise.all(
        document
          .getAnimations()
          .filter((animation) => animation instanceof CSSTransition)
          .map((animation) => animation.finished.catch(() => undefined)),
      ),
    );
    const since = (await reports()).slice(before);
    expect(since.map((report) => report.resultIndex + 1)).toEqual(since.map(() => index));
    await active(index);
    await expect(activeEditor).toBeFocused();
    expect(await total()).toBeGreaterThanOrEqual(2);
    // The active match was on screen before the resize, so it is brought back on screen after it; xterm
    // draws the selection only for visible rows (HS2-X96PS0).
    await expect(drawer.locator('.xterm-selection > div').first()).toBeVisible();
    await page.screenshot({ path: test.info().outputPath(`hs2-swy5dq-active-${index}-${width}.png`) });
  };

  // Narrow: the first match stays active; Enter steps to the second and Shift-Enter back.
  await resizeKeeping(390, 844, 1, true);
  await page.keyboard.press('Enter');
  await active(2);
  await page.keyboard.press('Shift+Enter');
  await active(1);
  await page.keyboard.press('Enter');
  await active(2);

  // Medium: the second match stays active and the steps go on from it, still in the full-screen bar
  // (full screen lasts below the desktop breakpoint).
  await resizeKeeping(768, 900, 2);
  await page.keyboard.press('Shift+Enter');
  await active(1);
  await page.keyboard.press('Enter');
  await active(2);

  // Wide again: full screen ends and the rail's field takes over the search and the focus; still the second
  // match; Enter moves past it (wrapping after the last) and Shift-Enter returns.
  await resizeKeeping(1280, 800, 2);
  await expect(editor).toBeFocused();
  const wideTotal = await total();
  await page.keyboard.press('Enter');
  await active(wideTotal > 2 ? 3 : 1);
  await page.keyboard.press('Shift+Enter');
  await active(2);

  // A step pressed straight after a resize, before the terminal re-searches, also goes on from the active
  // match instead of restarting from the first.
  await page.keyboard.press('Shift+Enter');
  await active(1);
  const before = (await reports()).length;
  // A tablet width keeps the rail's field (only a phone rail moves search to the full-screen bar).
  await page.setViewportSize({ width: 768, height: 900 });
  await page.keyboard.press('Enter');
  await active(2);
  await expect.poll(async () => (await reports()).length).toBeGreaterThan(before + 1);
  await active(2);
});

test('HS2-7HPXCY phone Search opens the full-screen search bar, steps through matches, and Exit keeps the search', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  await page.setViewportSize({ width: 390, height: 844 });
  const railSearch = drawer.getByRole('button', { name: 'Search Terminal' }),
    bar = page.locator('[data-component="terminal-focus-search"]'),
    editor = bar.locator('[data-search-field-editor="terminal-search"]'),
    results = bar
      .getByRole('toolbar', { name: 'Terminal Search Results' })
      .or(bar.locator('[aria-label="Terminal Search Results"]')),
    status = bar.locator('[data-terminal-focus-search-status]'),
    previous = bar.getByRole('button', { name: 'Previous Match' }),
    next = bar.getByRole('button', { name: 'Next Match' }),
    exit = bar.getByRole('button', { name: 'Exit terminal focus' }),
    highlights = viewport.locator('.xterm-find-result-decoration'),
    inViewport = async (...controls: ReturnType<typeof page.locator>[]) => {
      const width = page.viewportSize()!.width;
      for (const control of controls) {
        const box = (await control.boundingBox())!;
        expect(box.width).toBeGreaterThan(0);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(width);
      }
    };
  // The phone rail offers a Search button, not the field, inside the drawer's bounds (HS2-HPDX81).
  await expect(drawer.locator('.kui-toolbar-control-group[data-content="search"]')).toHaveCount(0);
  await expect(railSearch).toHaveAttribute('aria-pressed', 'false');
  await inViewport(railSearch, drawer.getByRole('button', { name: 'Hide Terminal Drawer' }));

  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    // Search opens the terminal full screen with the field focused; nothing typed, no results line.
    await railSearch.click();
    await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
    await expect(editor).toBeFocused();
    await expect(results).toHaveCount(0);
    await expect(page.locator('.terminal-drawer__focus-controls')).toHaveCount(0);
    await expect(page.getByRole('toolbar', { name: 'Terminal Focus Exit' })).toHaveCount(0);
    await inViewport(editor, exit);
    await page.screenshot({ path: test.info().outputPath(`hs2-7hpxcy-empty-390-${scheme}.png`) });

    // Typing shows the count line; Next and Previous step and keep typing in the field.
    await page.keyboard.type('File');
    await expect(status).toHaveText(/^1 of \d+$/);
    const total = Number((await status.textContent())!.split(' of ')[1]);
    expect(total).toBeGreaterThanOrEqual(2);
    await expect(results).toContainText(/1 of \d+/);
    const counted = async () => Number((await status.textContent())!.split(' of ')[1]);
    // xterm draws highlights only on the rows on screen, and the full-screen reflow can move a match into
    // scrollback, so count the matches on screen: one decoration each, plus one over the active match.
    const onScreen = async () =>
      (await viewport.locator('.xterm-rows > div').allTextContents()).join('\n').split('File').length - 1;
    await expect.poll(async () => (await highlights.count()) - (await onScreen())).toBe(1);
    // The fake PTY redraws at the full-screen size, so the total may change; the steps may not.
    await next.click();
    await expect(status).toHaveText(/^2 of \d+$/);
    await expect(editor).toBeFocused();
    await previous.click();
    await expect(status).toHaveText(/^1 of \d+$/);
    await next.click();
    await expect(status).toHaveText(/^2 of \d+$/);
    await inViewport(editor, exit, previous, next);
    await page.screenshot({ path: test.info().outputPath(`hs2-7hpxcy-matches-390-${scheme}.png`) });

    // No matches: the line says so and the steps are disabled.
    await page.keyboard.type('-absent');
    await expect(status).toHaveText('No matches');
    await expect(previous).toBeDisabled();
    await expect(next).toBeDisabled();
    await page.screenshot({ path: test.info().outputPath(`hs2-7hpxcy-none-390-${scheme}.png`) });
    for (let index = 0; index < '-absent'.length; index += 1) await page.keyboard.press('Backspace');
    await expect(status).toHaveText(/^1 of \d+$/);
    expect(await counted()).toBeGreaterThanOrEqual(2);

    // Exit leaves full screen but not the search: the drawer keeps the highlights, and the rail's
    // Search is pressed and focused.
    await exit.click();
    await expect(drawer).toHaveAttribute('data-focus-mode', 'false');
    await expect(bar).toHaveCount(0);
    await expect(railSearch).toBeFocused();
    await expect(railSearch).toHaveAttribute('aria-pressed', 'true');
    await expect(highlights).not.toHaveCount(0);
    await page.screenshot({ path: test.info().outputPath(`hs2-7hpxcy-exited-390-${scheme}.png`) });

    // Search again reopens the bar with the kept query.
    await railSearch.click();
    await expect(editor).toHaveText('File');
    await expect(status).toHaveText(/^\d+ of \d+$/);
    // Emptying the field hides the results line; Exit then ends the search.
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.press('Backspace');
    await expect(results).toHaveCount(0);
    await expect(highlights).toHaveCount(0);
    await exit.click();
    await expect(railSearch).toHaveAttribute('aria-pressed', 'false');
  }

  // Tap-to-focus full screen offers Search in its floating controls (answer 4), which opens the bar there.
  await page.emulateMedia({ colorScheme: 'light' });
  await viewport.locator('.xterm-helper-textarea').focus();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  const focusSearch = page.locator('.terminal-drawer__focus-controls').getByRole('button', { name: 'Search Terminal' });
  await expect(focusSearch).toBeVisible();
  await page.screenshot({ path: test.info().outputPath('hs2-7hpxcy-focus-search-button-390.png') });
  await focusSearch.click();
  await expect(editor).toBeFocused();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  await expect(page.locator('.terminal-drawer__focus-controls')).toHaveCount(0);
  await exit.click();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'false');
});
