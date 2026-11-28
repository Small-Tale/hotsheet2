import './announcer-overlay.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { ResizableRegion } from '@kerfjs/ui/resizable-region';
import { PinnablePopoverSurface } from '@kerfjs/ui/surface-scaffold';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import {
  AudioLines,
  ChevronLeft,
  ChevronRight,
  Gauge,
  ListX,
  Maximize2,
  Milestone,
  Minimize2,
  PictureInPicture,
  PictureInPicture2,
  Play,
  Radio,
  Settings,
  Square,
  Volume2,
  VolumeX,
} from 'lucide';

import { ANNOUNCER_DOCK_BOUNDS, ANNOUNCER_DOCK_REGION_ID, ANNOUNCER_FLOATING_ID } from '../announcer-layout';
import type { AnnouncerReelItem } from '../announcer-reel';
import { announcerSettingsMenu } from '../announcer-settings';
import type { AnnouncerAiSettings } from '../api';
import { MarkdownPreview } from './markdown-preview';

/** Where the open announcer shows (HS2-BS0XFK): `docked` in the left sidebar's footer under a resizable
 * separator, or `corner` as a card over the work area while that sidebar is not shown. `floating` is the
 * detached announcer (HS2-51X25Z): a persistent, movable, resizable Kerf PinnablePopoverSurface. */
export type AnnouncerPresentation = 'docked' | 'corner' | 'floating';

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
  /** The selected engine's voice names the settings menu offers (HS2-PRCC6V, HS2-CEZVSE). */
  voices?: readonly string[];
  /** Speech rate (HS2-PRCC6V). */
  rate?: number;
  /** The server's milestone AI switches (HS2-G3HJZH). */
  ai?: AnnouncerAiSettings;
  presentation?: AnnouncerPresentation;
  /** The docked section's height, as the device last stored it. */
  dockSize?: number;
  /** `embedded` draws the floating surface in flow, for the catalog only. */
  surfacePresentation?: 'overlay' | 'embedded';
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
  ai,
  presentation = 'corner',
  dockSize = ANNOUNCER_DOCK_BOUNDS.fallback,
  surfacePresentation = 'overlay',
}: AnnouncerOverlayProps) {
  if (!open) return <></>;
  const selected = items.at(index);
  const docked = presentation === 'docked';
  const floating = presentation === 'floating';
  // Expand is a detached-only control (HS2-51X25Z #10); attached placements keep their own size.
  const expanded = expandedRequest && floating;
  const count = items.length ? `${index + 1} / ${items.length}` : 'No activity';
  const announcer = (
    <aside
      class="announcer"
      data-component="announcer-overlay"
      data-presentation={presentation}
      data-expanded={String(expanded)}
    >
      <section class="announcer__panel" aria-label="Announcer">
        {/* Detached, the floating surface's own header carries the title and controls. Attached, the
            former expand control is the pop-out (HS2-51X25Z #3); the toolbar toggle hides it, so no X. */}
        {!floating && (
          <header class="announcer__header">
            <strong>Announcer</strong>
            <span class="announcer__count">{count}</span>
            <button type="button" aria-label="Pop Out Announcer" title="Pop out announcer" {...action('detach')}>
              <LucideIcon icon={PictureInPicture2} name="picture-in-picture-2" />
            </button>
          </header>
        )}
        <div class="announcer__screen" aria-live="polite">
          {selected ? (
            <>
              <div class="announcer__source">
                {selected.projectName} · {selected.event.tool}
              </div>
              {/* HS2-YJK0G9: the shared note renderer shows Markdown, with commands and code monospaced. */}
              <div class="announcer__summary">
                <MarkdownPreview source={selected.narration} density="compact" size="inherit" />
              </div>
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
                    ai,
                    icons: {
                      voice: <LucideIcon icon={AudioLines} name="audio-lines" />,
                      speed: <LucideIcon icon={Gauge} name="gauge" />,
                      milestones: <LucideIcon icon={Milestone} name="milestone" />,
                      clear: <LucideIcon icon={ListX} name="list-x" />,
                    },
                  })}
                />
              </ToolbarControlGroup>
            </>
          }
        />
      </section>
    </aside>
  );
  if (floating)
    return (
      <PinnablePopoverSurface
        id={ANNOUNCER_FLOATING_ID}
        label="Announcer"
        presentation={surfacePresentation}
        pinnable={false}
        closable={false}
        // Kerf's default description says moving pins it; a persistent surface is always pinned (KF-4T907F).
        moveDescription="Drag the title or press the arrow keys to move it; hold Shift to move farther."
        preferredWidth={expanded ? remify(960) : remify(320)}
        height={expanded ? remify(560) : remify(320)}
        minHeight={remify(200)}
        bodyLayout="fill"
        bodyInset="none"
        rootAttributes={{
          'data-announcer-floating': '',
          'data-expanded': String(expanded),
        }}
        headerActions={
          <>
            <ToolbarText text={count} size="small" />
            {/* Icon-only Kerf Buttons: the composition rule keeps ToolbarControlGroup inside a Toolbar
                (KUI-L201), though Kerf's persistent-panel example puts one here (KF-SMTCPZ). */}
            <Button
              emphasis="plain"
              size="compact"
              ariaLabel={expanded ? 'Shrink Announcer' : 'Expand Announcer'}
              icon={
                <LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} />
              }
              rootAttributes={action('expand')}
            />
            <Button
              emphasis="plain"
              size="compact"
              ariaLabel="Reattach Announcer"
              title="Reattach to the sidebar"
              icon={<LucideIcon icon={PictureInPicture} name="picture-in-picture" />}
              rootAttributes={action('reattach')}
            />
          </>
        }
      >
        {announcer}
      </PinnablePopoverSurface>
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
