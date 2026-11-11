import type { SafeHtml } from 'kerfjs';

import type { ThemePreference } from './theme-preference';

/**
 * The stylized Hot Sheet windows on the Theme picker (HS2-EFTHS6), after news-cloud2's Appearance tiles:
 * a title bar, a project sidebar with one selected view, and three ticket columns with cards. Each tile
 * shows its own scheme whatever the app wears, so the palettes are frozen literals rather than theme
 * tokens; the Auto tile splits light and dark on a diagonal, the way macOS draws its Auto appearance.
 */
export interface TilePalette {
  /** Window and card fill. */
  bg: string;
  /** Title bar, sidebar, and column wells. */
  soft: string;
  /** Text bars, drawn at reduced opacity. */
  text: string;
  /** The selected view and a card's category edge. */
  accent: string;
}

/** Light canvas, grouped fill, label, and system blue, matching the light `theme-color` (#ffffff). */
export const LIGHT_TILE: TilePalette = { bg: '#ffffff', soft: '#f2f2f7', text: '#1d1d1f', accent: '#0a72ef' };
/** Dark canvas, grouped fill, label, and blue, matching the dark `theme-color` (#1c1c1e). */
export const DARK_TILE: TilePalette = { bg: '#1c1c1e', soft: '#2c2c2e', text: '#f2f2f7', accent: '#3d8bff' };

/** One ticket card: a bold title bar over a thin line, with an optional accent category edge. */
function card(p: TilePalette, x: number, y: number, accent: boolean): SafeHtml {
  return (
    <g>
      <rect x={x} y={y} width="24" height="15" rx="2.5" fill={p.bg} stroke={p.text} strokeOpacity="0.14" />
      {accent ? <rect x={x} y={y + 2} width="2" height="11" rx="1" fill={p.accent} /> : ''}
      <rect x={x + 4} y={y + 4} width="16" height="3" rx="1.5" fill={p.text} opacity="0.6" />
      <rect x={x + 4} y={y + 9.5} width="12" height="2" rx="1" fill={p.text} opacity="0.25" />
    </g>
  );
}

/** The window mockup in one palette, in the shared 132×92 viewBox. */
function mockup(p: TilePalette): SafeHtml {
  return (
    <g>
      <rect x="0" y="0" width="132" height="92" fill={p.bg} />
      <rect x="0" y="0" width="132" height="12" fill={p.soft} />
      <circle cx="7" cy="6" r="2" fill={p.text} opacity="0.3" />
      <circle cx="13" cy="6" r="2" fill={p.text} opacity="0.3" />
      <circle cx="19" cy="6" r="2" fill={p.text} opacity="0.3" />
      <rect x="0" y="12" width="34" height="80" fill={p.soft} />
      <rect x="6" y="20" width="22" height="5" rx="2.5" fill={p.accent} />
      <rect x="6" y="30" width="22" height="4" rx="2" fill={p.text} opacity="0.3" />
      <rect x="6" y="39" width="22" height="4" rx="2" fill={p.text} opacity="0.3" />
      <rect x="6" y="48" width="16" height="4" rx="2" fill={p.text} opacity="0.3" />
      {[39, 70, 101].map((x) => (
        <rect x={x + 1} y="18" width="14" height="3" rx="1.5" fill={p.text} opacity="0.45" />
      ))}
      {card(p, 39, 26, true)}
      {card(p, 39, 45, false)}
      {card(p, 39, 64, false)}
      {card(p, 70, 26, false)}
      {card(p, 70, 45, true)}
      {card(p, 101, 26, false)}
    </g>
  );
}

/**
 * One tile's SVG, decorative (`aria-hidden`): the radio's visible label names the choice. Clip ids carry
 * the kind so the three tiles can share a document.
 */
export function themeTileArt(kind: ThemePreference): SafeHtml {
  const clip = `theme-tile-clip-${kind}`;
  return (
    <svg viewBox="0 0 132 92" aria-hidden="true">
      <defs>
        <clipPath id={clip}>
          <rect x="0" y="0" width="132" height="92" rx="8" />
        </clipPath>
        {kind === 'auto' ? (
          <g>
            <clipPath id={`${clip}-light`}>
              <polygon points="0,0 76,0 52,92 0,92" />
            </clipPath>
            <clipPath id={`${clip}-dark`}>
              <polygon points="76,0 132,0 132,92 52,92" />
            </clipPath>
          </g>
        ) : (
          ''
        )}
      </defs>
      {kind === 'auto' ? (
        <g clipPath={`url(#${clip})`}>
          <g clipPath={`url(#${clip}-light)`}>{mockup(LIGHT_TILE)}</g>
          <g clipPath={`url(#${clip}-dark)`}>{mockup(DARK_TILE)}</g>
        </g>
      ) : (
        <g clipPath={`url(#${clip})`}>{mockup(kind === 'dark' ? DARK_TILE : LIGHT_TILE)}</g>
      )}
    </svg>
  );
}
