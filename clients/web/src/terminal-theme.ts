import type { ITheme } from '@xterm/xterm';

/**
 * Light and dark xterm themes (HS2-QNSS9C). Terminals follow the app's light/dark appearance: the
 * surface colors come from the `--hs-terminal-background` / `--hs-terminal-foreground` tokens, which
 * are `light-dark()` pairs resolved by the element's color scheme, and the cursor and selection derive
 * from them. The ANSI palette is chosen by the resolved background: the dark scheme keeps xterm's
 * stock palette (unchanged from before), and the light scheme uses a palette whose colors stay legible
 * on a white surface (xterm's stock yellow, cyan, and bright colors nearly vanish there).
 */

export type TerminalScheme = 'light' | 'dark';

/** ANSI colors tuned for a light terminal surface. */
export const LIGHT_TERMINAL_ANSI: Readonly<ITheme> = Object.freeze({
  black: '#000000',
  red: '#c42b1c',
  green: '#0b7a0b',
  yellow: '#7a6400',
  blue: '#0451a5',
  magenta: '#a3139e',
  cyan: '#0a7389',
  white: '#6e6e73',
  brightBlack: '#5a5a5f',
  brightRed: '#d13438',
  brightGreen: '#107c10',
  brightYellow: '#8a6d00',
  brightBlue: '#0f6cbd',
  brightMagenta: '#b4009e',
  brightCyan: '#038387',
  brightWhite: '#8e8e93',
});

type Rgba = readonly [number, number, number, number];

/** Parse a computed `rgb()`/`rgba()` color (the form getComputedStyle returns for sRGB colors). */
export function parseComputedColor(value: string): Rgba | undefined {
  const match = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+%?))?\s*\)$/i.exec(value.trim());
  if (!match) return undefined;
  const alphaText = match[4] as string | undefined,
    alpha =
      alphaText === undefined ? 1 : alphaText.endsWith('%') ? Number(alphaText.slice(0, -1)) / 100 : Number(alphaText);
  return [Number(match[1]), Number(match[2]), Number(match[3]), alpha];
}

function relativeLuminance([red, green, blue]: Rgba): number {
  const channel = (value: number) => {
    const srgb = value / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue);
}

/** The scheme a resolved terminal background belongs to. */
export function terminalSchemeFor(background: string): TerminalScheme {
  const rgba = parseComputedColor(background);
  return rgba && relativeLuminance(rgba) > 0.5 ? 'light' : 'dark';
}

/** Build the xterm theme from the resolved surface colors. */
export function buildTerminalTheme(background: string, foreground: string): ITheme {
  const scheme = terminalSchemeFor(background),
    rgba = parseComputedColor(foreground),
    selectionBackground = rgba
      ? `rgba(${rgba[0]}, ${rgba[1]}, ${rgba[2]}, ${scheme === 'light' ? 0.22 : 0.3})`
      : undefined;
  return {
    background,
    foreground,
    cursor: foreground,
    cursorAccent: background,
    ...(selectionBackground ? { selectionBackground } : {}),
    ...(scheme === 'light' ? LIGHT_TERMINAL_ANSI : {}),
  };
}

/**
 * Resolve the terminal tokens for `element`. Custom properties report their specified `light-dark()`
 * text, so each token is resolved through a probe's computed `color`, which applies the scheme.
 */
export function resolveTerminalTheme(element: HTMLElement): ITheme {
  const probe = element.ownerDocument.createElement('span');
  probe.hidden = true;
  element.append(probe);
  try {
    const resolve = (token: string, fallback: string) => {
      probe.style.color = `var(${token}, ${fallback})`;
      return getComputedStyle(probe).color || fallback;
    };
    return buildTerminalTheme(resolve('--hs-terminal-background', '#000'), resolve('--hs-terminal-foreground', '#fff'));
  } finally {
    probe.remove();
  }
}

/**
 * Call `apply` whenever the app's light/dark appearance may have changed: the system preference, or the
 * root's theme attribute/class (the explicit `data-theme` choice and Web Awesome's `wa-dark`). Returns
 * a disposer.
 */
export function watchTerminalScheme(apply: () => void, root: HTMLElement = document.documentElement): () => void {
  const media = window.matchMedia('(prefers-color-scheme: dark)'),
    observer = new MutationObserver(apply);
  media.addEventListener('change', apply);
  observer.observe(root, { attributes: true, attributeFilter: ['class', 'data-theme', 'style'] });
  return () => {
    media.removeEventListener('change', apply);
    observer.disconnect();
  };
}

/** Theme a mounted terminal now and keep it following the app's appearance until disposed. */
export function followTerminalTheme(element: HTMLElement, terminal: { options: { theme?: ITheme } }): () => void {
  let current = JSON.stringify(terminal.options.theme ?? {});
  return watchTerminalScheme(() => {
    if (!element.isConnected) return;
    const next = resolveTerminalTheme(element),
      serialized = JSON.stringify(next);
    if (serialized === current) return;
    current = serialized;
    terminal.options.theme = next;
  });
}
