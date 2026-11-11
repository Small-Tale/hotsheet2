import './announcer-overlay.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { ResizableRegion } from '@kerfjs/ui/resizable-region';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import {
  AudioLines,
  ChevronLeft,
  ChevronRight,
  Gauge,
  ListX,
  Maximize2,
  Minimize2,
  Play,
  Radio,
  Settings,
  Square,
  Volume2,
  VolumeX,
} from 'lucide';

import { ANNOUNCER_DOCK_BOUNDS, ANNOUNCER_DOCK_REGION_ID } from '../announcer-layout';
import type { AnnouncerReelItem } from '../announcer-reel';
import { announcerSettingsMenu } from '../announcer-settings';

/** Where the open announcer shows (HS2-BS0XFK): `docked` in the left sidebar's footer under a resizable
 * separator, or `corner` as a card over the work area while that sidebar is not shown. */
export type AnnouncerPresentation = 'docked' | 'corner';

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
  /** Browser voice names the settings menu offers (HS2-PRCC6V). */
  voices?: readonly string[];
  /** Speech rate (HS2-PRCC6V). */
  rate?: number;
  presentation?: AnnouncerPresentation;
  /** The docked section's height, as the device last stored it. */
  dockSize?: number;
}

const action = (name: string) => ({ 'data-announcer-action': name });

/** The retained digest and live reel occupy one movable-size picture-in-picture surface
 * (HS2-5PSQJQ). The parent owns playback state; this component only renders it. */
export function AnnouncerOverlay({
  open,
  expanded: expandedRequest,
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
  voices = [],
  rate = 1,
  presentation = 'corner',
  dockSize = ANNOUNCER_DOCK_BOUNDS.fallback,
}: AnnouncerOverlayProps) {
  if (!open) return <></>;
  const selected = items.at(index);
  const docked = presentation === 'docked';
  // The dock's height is the region's, so only the corner card grows when expanded.
  const expanded = expandedRequest && !docked;
  const announcer = (
    <aside
      class="announcer"
      data-component="announcer-overlay"
      data-presentation={presentation}
      data-expanded={String(expanded)}
    >
      <section class="announcer__panel" aria-label="Announcer">
        <header class="announcer__header">
          <strong>Announcer</strong>
          <span class="announcer__count">{items.length ? `${index + 1} / ${items.length}` : 'No activity'}</span>
          {!docked && (
            <button type="button" aria-label={expanded ? 'Shrink Announcer' : 'Expand Announcer'} {...action('expand')}>
              <LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} />
            </button>
          )}
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
        {/* HS2-YF8P38: playback stays together at the start and the narration toggles at the end; spare
            width goes between the groups. HS2-EYW4H8: the toggles are Kerf pressed controls whose raised
            chrome, aria-pressed state, and icon (Audio) show on or off without relying on color. They are
            icon-only so both groups share one row in the narrowest docked sidebar (250px). */}
        <Toolbar
          label="Announcer Controls"
          position="footer"
          density="compact"
          dividerSides="t"
          responsive="wrap-groups"
          leading={
            <ToolbarControlGroup size="compact" density="compact" label="Playback">
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
            </ToolbarControlGroup>
          }
          trailing={
            <>
              <ToolbarControlGroup size="compact" density="compact" label="Narration">
                <button type="button" aria-label="Live" aria-pressed={String(live)} {...action('live')}>
                  <LucideIcon icon={Radio} name="radio" />
                </button>
                <button type="button" aria-label="Audio" aria-pressed={String(audible)} {...action('audio')}>
                  <LucideIcon icon={audible ? Volume2 : VolumeX} name={audible ? 'volume-2' : 'volume-x'} />
                </button>
              </ToolbarControlGroup>
              {/* HS2-PRCC6V: voice, speed, and clearing history live in one settings menu beside Audio. */}
              <ToolbarControlGroup size="compact" density="compact" single nestedDropdown label="Announcer Settings">
                <PopupMenu
                  label="Announcer Settings"
                  icon={<LucideIcon icon={Settings} name="settings" />}
                  caret={false}
                  placement="top-end"
                  rootAttributes={{ 'data-announcer-settings': '' }}
                  items={announcerSettingsMenu({
                    providers,
                    provider,
                    voices,
                    voice,
                    rate,
                    hasHistory: items.length > 0,
                    icons: {
                      voice: <LucideIcon icon={AudioLines} name="audio-lines" />,
                      speed: <LucideIcon icon={Gauge} name="gauge" />,
                      clear: <LucideIcon icon={ListX} name="list-x" />,
                    },
                  })}
                />
              </ToolbarControlGroup>
            </>
          }
        />
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
  if (!docked) return announcer;
  // Kerf's region owns the separator line above the section and its pointer and keyboard resizing.
  return (
    <ResizableRegion
      id={ANNOUNCER_DOCK_REGION_ID}
      label="Announcer Dock"
      axis="vertical"
      edge="start"
      size={dockSize}
      min={ANNOUNCER_DOCK_BOUNDS.min}
      max={ANNOUNCER_DOCK_BOUNDS.max}
    >
      {announcer}
    </ResizableRegion>
  );
}
