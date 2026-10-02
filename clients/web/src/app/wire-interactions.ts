import { combineInteractionTeardowns, type InteractionTeardown } from '../interactions/lifetime';

/**
 * Ordered registration callbacks for the application's delegated and native handlers. Each
 * returns the teardown that removes every listener its group registered.
 */
export interface HotSheetInteractionRegistrations {
  projectLifecycle(): InteractionTeardown;
  repository(): InteractionTeardown;
  navigationAndTabs(): InteractionTeardown;
  terminals(): InteractionTeardown;
  ticketSelection(): InteractionTeardown;
  viewsAndSavedViews(): InteractionTeardown;
  commandsAndAi(): InteractionTeardown;
  notificationsAndLinks(): InteractionTeardown;
  searchAndComposer(): InteractionTeardown;
  attachmentsAndGallery(): InteractionTeardown;
  inspectorAndEditor(): InteractionTeardown;
  shellAndGlobal(): InteractionTeardown;
}

/**
 * Install every interaction group once in the established behavior-sensitive order.
 * Keeping the sequence here makes the application bootstrap independent of selectors
 * and event-registration details owned by the interaction modules. Returns one
 * page-lifetime teardown that removes every group's listeners (newest group first).
 */
export function wireHotSheetInteractions(registrations: HotSheetInteractionRegistrations): InteractionTeardown {
  const teardowns: InteractionTeardown[] = [];
  teardowns.push(registrations.projectLifecycle());
  teardowns.push(registrations.repository());
  teardowns.push(registrations.navigationAndTabs());
  teardowns.push(registrations.terminals());
  teardowns.push(registrations.ticketSelection());
  teardowns.push(registrations.viewsAndSavedViews());
  teardowns.push(registrations.commandsAndAi());
  teardowns.push(registrations.notificationsAndLinks());
  teardowns.push(registrations.searchAndComposer());
  teardowns.push(registrations.attachmentsAndGallery());
  teardowns.push(registrations.inspectorAndEditor());
  teardowns.push(registrations.shellAndGlobal());
  return combineInteractionTeardowns(teardowns);
}
