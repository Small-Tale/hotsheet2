import './announcer-overlay.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronLeft, ChevronRight, Maximize2, Minimize2, Play, Radio, Square, Volume2, X } from 'lucide';

import type { AnnouncerReelItem } from '../announcer-reel';

export interface AnnouncerOverlayProps {
  open: boolean;
  expanded: boolean;
  live: boolean;
  speaking: boolean;
  audible: boolean;
  items: readonly AnnouncerReelItem[];
  index: number;
  error?: string;
  currentDiff?: { key: string; diff: string; truncated: boolean };
  diffLoading?: boolean;
  diffError?: string;
  providers?: readonly string[];
  provider?: string;
  voice?: string;
}

const action = (name: string) => ({ 'data-announcer-action': name });

/** The retained digest and live reel occupy one movable-size picture-in-picture surface
 * (HS2-5PSQJQ). The parent owns playback state; this component only renders it. */
export function AnnouncerOverlay({
  open,
  expanded,
  live,
  speaking,
  audible,
  items,
  index,
  error,
  currentDiff,
  diffLoading = false,
  diffError,
  providers = [],
  provider = 'browser',
  voice = '',
}: AnnouncerOverlayProps) {
  if (!open) return <></>;
  const selected = items.at(index);
  return (
    <aside class="announcer" data-component="announcer-overlay" data-expanded={String(expanded)}>
      <section class="announcer__panel" aria-label="Announcer">
        <header class="announcer__header">
          <strong>Announcer</strong>
          <span class="announcer__count">{items.length ? `${index + 1} / ${items.length}` : 'No activity'}</span>
          <button type="button" aria-label={expanded ? 'Shrink Announcer' : 'Expand Announcer'} {...action('expand')}>
            <LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} />
          </button>
          <button type="button" aria-label="Close Announcer" {...action('close')}>
            <LucideIcon icon={X} name="x" />
          </button>
        </header>
        <div class="announcer__screen" aria-live="polite">
          {selected ? (
            <>
              <div class="announcer__source">
                {selected.projectName} · {selected.event.tool}
              </div>
              <p class="announcer__summary">{selected.narration}</p>
              {selected.change && (
                <div class="announcer__change">
                  <code>{selected.change.path}</code>
                  {selected.change.operation && <span>{selected.change.operation}</span>}
                  {selected.change.fileCount && selected.change.fileCount > 1 && (
                    <span>{selected.change.fileCount} files in edit</span>
                  )}
                  <button type="button" disabled={diffLoading} {...action('current-diff')}>
                    {diffLoading ? 'Loading current diff…' : 'View current diff'}
                  </button>
                  {currentDiff?.key === selected.key && (
                    <div>
                      <span>Current working-tree diff · may differ from this edit</span>
                      {currentDiff.diff ? (
                        <pre aria-label="Current working-tree diff">{currentDiff.diff}</pre>
                      ) : (
                        <p>No current tracked changes for this file.</p>
                      )}
                      {currentDiff.truncated && <p>Diff truncated at 64 KiB.</p>}
                    </div>
                  )}
                  {diffError && <p role="alert">{diffError}</p>}
                </div>
              )}
            </>
          ) : (
            <p class="announcer__empty">Activity from open projects will appear here.</p>
          )}
        </div>
        {error && (
          <p class="announcer__error" role="alert">
            {error}
          </p>
        )}
        <footer class="announcer__controls">
          <button type="button" aria-label="Previous Activity" disabled={index <= 0} {...action('previous')}>
            <LucideIcon icon={ChevronLeft} name="chevron-left" />
          </button>
          <button
            type="button"
            aria-label={speaking ? 'Stop Narration' : 'Play Narration'}
            disabled={!selected}
            {...action('play')}
          >
            <LucideIcon icon={speaking ? Square : Play} name={speaking ? 'square' : 'play'} />
          </button>
          <button type="button" aria-label="Next Activity" disabled={index >= items.length - 1} {...action('next')}>
            <LucideIcon icon={ChevronRight} name="chevron-right" />
          </button>
          <button type="button" aria-pressed={live} {...action('live')}>
            <LucideIcon icon={Radio} name="radio" /> Live
          </button>
          <button type="button" aria-pressed={audible} {...action('audio')}>
            <LucideIcon icon={Volume2} name="volume-2" /> Audio
          </button>
        </footer>
        <label class="announcer__voice">
          Voice
          <select aria-label="Announcer voice provider" data-announcer-provider>
            <option value="browser" selected={provider === 'browser'}>
              Browser voice
            </option>
            {providers.map((id) => (
              <option value={id} selected={provider === id}>
                {id === 'macos-say' ? 'Mac voice on server' : id}
              </option>
            ))}
          </select>
        </label>
        {expanded && (
          <label class="announcer__voice">
            Voice name
            <input data-announcer-voice value={voice} placeholder="Default voice" aria-label="Announcer voice name" />
          </label>
        )}
        {expanded && (
          <ol class="announcer__reel" aria-label="Cross-Project Activity Reel">
            {items.map((item, itemIndex) => (
              <li data-current={String(itemIndex === index)}>
                <button type="button" data-announcer-action="select" data-index={itemIndex}>
                  <span>{item.projectName}</span> {item.narration}
                </button>
              </li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
