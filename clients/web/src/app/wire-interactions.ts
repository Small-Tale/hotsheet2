/** Ordered registration callbacks for the application's delegated and native handlers. */
export interface HotSheetInteractionRegistrations {
  projectLifecycle(): void;
  repository(): void;
  navigationAndTabs(): void;
  terminals(): void;
  ticketSelection(): void;
  viewsAndSavedViews(): void;
  commandsAndAi(): void;
  notificationsAndLinks(): void;
  searchAndComposer(): void;
  attachmentsAndGallery(): void;
  inspectorAndEditor(): void;
  shellAndGlobal(): void;
}

/**
 * Install every interaction group once in the established behavior-sensitive order.
 * Keeping the sequence here makes the application bootstrap independent of selectors
 * and event-registration details owned by the interaction modules.
 */
export function wireHotSheetInteractions(registrations: HotSheetInteractionRegistrations) {
  registrations.projectLifecycle();
  registrations.repository();
  registrations.navigationAndTabs();
  registrations.terminals();
  registrations.ticketSelection();
  registrations.viewsAndSavedViews();
  registrations.commandsAndAi();
  registrations.notificationsAndLinks();
  registrations.searchAndComposer();
  registrations.attachmentsAndGallery();
  registrations.inspectorAndEditor();
  registrations.shellAndGlobal();
}
