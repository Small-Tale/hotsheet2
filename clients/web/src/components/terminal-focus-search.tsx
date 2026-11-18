import '@awesome.me/webawesome/dist/styles/utilities/visually-hidden.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { SearchField } from '@kerfjs/ui/search-field';
import { type SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ChevronDown, ChevronUp, Minimize2 } from 'lucide';

import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import { TERMINAL_SEARCH_FIELD_ID } from '../terminal-search';

export interface TerminalFocusSearchProps {
  /** The terminal search's shared field model, so the drawer's highlights and count follow this field. */
  model: SearchFieldModel;
  /** The searched terminal's match count ("3 of 12", "No matches"); undefined until it answers. */
  count?: string;
  /** Whether there is a match to step to; Previous and Next are disabled without one. */
  hasMatches?: boolean;
  /** The results line shows only while the field has a query (HS2-7HPXCY, answer 5). */
  hasQuery?: boolean;
}

/**
 * The phone full-screen terminal's search bar (HS2-7HPXCY, HS2-HPDX81 Design v1 variant A). The first
 * line gives the search field the whole bar, with Exit at its end; the second line, shown once there is
 * a query, holds the match count with Previous and Next at its end. Exit leaves full screen but keeps
 * the search, whose highlights stay in the drawer.
 */
export function TerminalFocusSearch({ model, count, hasMatches = false, hasQuery = false }: TerminalFocusSearchProps) {
  return (
    <div class="terminal-focus-search" data-component="terminal-focus-search">
      <Toolbar
        label="Terminal Search"
        density="compact"
        centerAlign="stretch"
        center={
          // Grow from a small basis so the field takes the bar beside Exit (Design v1, line 1).
          <ToolbarControlGroup
            size="compact"
            single
            expanded
            content="search"
            focusRing="halo"
            sizing="grow"
            growBasis="8rem"
          >
            <SearchField
              presentation="toolbar-group"
              id={TERMINAL_SEARCH_FIELD_ID}
              label="Search Terminal"
              model={model}
              placeholder="Search terminal"
              clearLabel="Clear Terminal Search"
            />
          </ToolbarControlGroup>
        }
        trailing={
          <ToolbarControlGroup size="compact" appearance="borderless" single content="mixed">
            <button
              type="button"
              {...TERMINALS_ACTIONS.exitTerminalFocusMode.attrs}
              aria-label="Exit terminal focus"
              title="Exit terminal focus"
            >
              <LucideIcon size="s" icon={Minimize2} name="minimize-2" />
              <span>Exit</span>
            </button>
          </ToolbarControlGroup>
        }
      />
      {hasQuery && (
        <Toolbar
          label="Terminal Search Results"
          density="compact"
          leading={<ToolbarText text={count ?? ''} size="small" />}
          trailing={
            <ToolbarControlGroup size="compact" appearance="borderless" content="icon" label="Matches">
              <button
                type="button"
                {...TERMINALS_ACTIONS.previousTerminalSearchMatch.attrs}
                disabled={!hasMatches}
                aria-label="Previous Match"
                title="Previous match"
              >
                <LucideIcon icon={ChevronUp} name="chevron-up" />
              </button>
              <button
                type="button"
                {...TERMINALS_ACTIONS.nextTerminalSearchMatch.attrs}
                disabled={!hasMatches}
                aria-label="Next Match"
                title="Next match"
              >
                <LucideIcon icon={ChevronDown} name="chevron-down" />
              </button>
            </ToolbarControlGroup>
          }
        />
      )}
      {/* ToolbarText is identity text, so the count is announced through a polite status of its own
          until Kerf's SearchField resultText reaches the installed release (KF-8CDT8B). */}
      <span class="wa-visually-hidden" role="status" data-terminal-focus-search-status="">
        {hasQuery ? (count ?? '') : ''}
      </span>
    </div>
  );
}
