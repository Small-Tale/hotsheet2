import { wireAttachmentAndGalleryInteractions } from '../interactions/attachments-and-gallery';
import { wireCommandAndAiInteractions } from '../interactions/commands-and-ai';
import { wireInspectorAndEditorInteractions } from '../interactions/inspector-and-editor';
import { wireNavigationAndTabInteractions } from '../interactions/navigation-and-tabs';
import { wireNotificationAndLinkInteractions } from '../interactions/notifications-and-links';
import { wireProjectLifecycleInteractions } from '../interactions/project-lifecycle';
import { wireRepositoryInteractions } from '../interactions/repository';
import { wireSearchAndComposerInteractions } from '../interactions/search-and-composer';
import { wireShellAndGlobalInteractions } from '../interactions/shell-and-global';
import { wireTerminalInteractions } from '../interactions/terminals';
import { wireTicketSelectionInteractions } from '../interactions/ticket-selection';
import { wireViewAndSavedViewInteractions } from '../interactions/views-and-saved-views';
import type { HotSheetInteractionRegistrations } from './wire-interactions';

type InteractionDependencies = Parameters<typeof wireProjectLifecycleInteractions>[0] &
  Parameters<typeof wireRepositoryInteractions>[0] &
  Parameters<typeof wireNavigationAndTabInteractions>[0] &
  Parameters<typeof wireTerminalInteractions>[0] &
  Parameters<typeof wireTicketSelectionInteractions>[0] &
  Parameters<typeof wireViewAndSavedViewInteractions>[0] &
  Parameters<typeof wireCommandAndAiInteractions>[0] &
  Parameters<typeof wireNotificationAndLinkInteractions>[0] &
  Parameters<typeof wireSearchAndComposerInteractions>[0] &
  Parameters<typeof wireAttachmentAndGalleryInteractions>[0] &
  Parameters<typeof wireInspectorAndEditorInteractions>[0] &
  Parameters<typeof wireShellAndGlobalInteractions>[0];

type OwnedInteractionState =
  | 'clearCommandDrag'
  | 'clearCommandDropIndicators'
  | 'commandLongPressFired'
  | 'commandLongPressTimer'
  | 'draggedCommandIds'
  | 'draggedGroupedAttachmentId'
  | 'draggedTickets';

/** Live application dependencies consumed by the interaction registration groups. */
export type InteractionBindingsPort = Omit<InteractionDependencies, OwnedInteractionState>;

/**
 * Bind the application's live dependency port to its ordered interaction registrations.
 * Mutable gesture state stays private here while inherited port accessors preserve the
 * runtime's getter/setter semantics for state shared with controllers and rendering.
 */
export function createHotSheetInteractionBindings(port: InteractionBindingsPort): HotSheetInteractionRegistrations {
  let commandLongPressTimer: number | undefined;
  let commandLongPressFired = false;
  let draggedCommandIds: string[] = [];
  let draggedGroupedAttachmentId: InteractionDependencies['draggedGroupedAttachmentId'];
  let draggedTickets: InteractionDependencies['draggedTickets'];

  function clearCommandDropIndicators() {
    document
      .querySelectorAll<HTMLElement>('[data-command-drop-position]')
      .forEach((element) => delete element.dataset.commandDropPosition);
    document
      .querySelectorAll<HTMLElement>('[data-command-drop-active]')
      .forEach((element) => delete element.dataset.commandDropActive);
  }

  function clearCommandDrag() {
    draggedCommandIds = [];
    document
      .querySelectorAll<HTMLElement>('[data-command-dragging]')
      .forEach((element) => delete element.dataset.commandDragging);
    clearCommandDropIndicators();
  }

  const dependencies = Object.create(port) as InteractionDependencies;
  Object.defineProperties(dependencies, {
    clearCommandDrag: { value: clearCommandDrag },
    clearCommandDropIndicators: { value: clearCommandDropIndicators },
    commandLongPressFired: {
      get: () => commandLongPressFired,
      set: (value: boolean) => {
        commandLongPressFired = value;
      },
    },
    commandLongPressTimer: {
      get: () => commandLongPressTimer,
      set: (value: number | undefined) => {
        commandLongPressTimer = value;
      },
    },
    draggedCommandIds: {
      get: () => draggedCommandIds,
      set: (value: string[]) => {
        draggedCommandIds = value;
      },
    },
    draggedGroupedAttachmentId: {
      get: () => draggedGroupedAttachmentId,
      set: (value: InteractionDependencies['draggedGroupedAttachmentId']) => {
        draggedGroupedAttachmentId = value;
      },
    },
    draggedTickets: {
      get: () => draggedTickets,
      set: (value: InteractionDependencies['draggedTickets']) => {
        draggedTickets = value;
      },
    },
  });

  return {
    projectLifecycle: () => {
      wireProjectLifecycleInteractions(dependencies);
    },
    repository: () => {
      wireRepositoryInteractions(dependencies);
    },
    navigationAndTabs: () => {
      wireNavigationAndTabInteractions(dependencies);
    },
    terminals: () => {
      wireTerminalInteractions(dependencies);
    },
    ticketSelection: () => {
      wireTicketSelectionInteractions(dependencies);
    },
    viewsAndSavedViews: () => {
      wireViewAndSavedViewInteractions(dependencies);
    },
    commandsAndAi: () => {
      wireCommandAndAiInteractions(dependencies);
    },
    notificationsAndLinks: () => {
      wireNotificationAndLinkInteractions(dependencies);
    },
    searchAndComposer: () => {
      wireSearchAndComposerInteractions(dependencies);
    },
    attachmentsAndGallery: () => {
      wireAttachmentAndGalleryInteractions(dependencies);
    },
    inspectorAndEditor: () => {
      wireInspectorAndEditorInteractions(dependencies);
    },
    shellAndGlobal: () => {
      wireShellAndGlobalInteractions(dependencies);
    },
  };
}
