import './project-close-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { deviceClass } from '@kerfjs/ui/device-class';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { CircleAlert, MessageSquare, SquareTerminal } from 'lucide';

import { type ConversationActivity, type ConversationMessage, type ConversationUsage } from '../ai-conversation';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import { AIConversation } from './ai-conversation';
import { TerminalPreview } from './terminal-dashboard';

interface ProjectCloseResourceBase {
  id: string;
  name: string;
  busy?: boolean;
  preview?: string;
}

export interface ProjectCloseTerminal extends ProjectCloseResourceBase {
  kind: 'terminal';
  cwd?: string;
  progress?: number;
}

export interface ProjectCloseAIChat extends ProjectCloseResourceBase {
  kind: 'ai-chat';
  tool: string;
  model?: string;
  effort?: string;
  sessionId?: string;
  messages: ConversationMessage[];
  activity?: ConversationActivity[];
  progress?: string;
  totalUsage?: ConversationUsage;
  error?: string;
}

export type ProjectCloseResource = ProjectCloseTerminal | ProjectCloseAIChat;

// Match the dialog's 672px resource-layout breakpoint when choosing its 8px phone gutter.
const projectCloseDevice = deviceClass({ breakpoints: { tablet: 673 } });

export interface ProjectCloseDialogState {
  projectId: string;
  projectName: string;
  resources: readonly ProjectCloseResource[];
  selectedKey?: string;
  operation?: 'closing-all' | 'closing-project';
  error?: string;
}

export function projectCloseResourceKey(resource: Pick<ProjectCloseResource, 'kind' | 'id'>): string {
  return `${resource.kind}:${resource.id}`;
}

export function selectedProjectCloseResource(
  resources: readonly ProjectCloseResource[],
  selectedKey?: string,
): ProjectCloseResource | undefined {
  return resources.find((resource) => projectCloseResourceKey(resource) === selectedKey) ?? resources[0];
}

export function projectCloseRunningSummary(resources: readonly ProjectCloseResource[]): string {
  const terminals = resources.filter((resource) => resource.kind === 'terminal').length,
    chats = resources.length - terminals,
    parts = [
      ...(terminals ? [`${terminals} running terminal${terminals === 1 ? '' : 's'}`] : []),
      ...(chats ? [`${chats} AI chat${chats === 1 ? '' : 's'}`] : []),
    ];
  return parts.length
    ? `${parts.join(' and ')} will stay active unless you close them first.`
    : 'No terminals or AI chats are currently running for this project.';
}

function ResourceDetail({ resource, projectId }: { resource: ProjectCloseResource; projectId: string }) {
  const key = projectCloseResourceKey(resource);
  if (resource.kind === 'terminal')
    return (
      <section
        class="project-close-dialog__detail project-close-dialog__terminal"
        data-key={`project-close-preview:${key}`}
        aria-label={`${resource.name} Terminal Preview`}
      >
        <TerminalPreview projectId={projectId} terminalId={resource.id} viewportKey={`project-close:${resource.id}`} />
      </section>
    );
  return (
    <section
      class="project-close-dialog__detail project-close-dialog__chat"
      data-key={`project-close-preview:${key}`}
      aria-label={`${resource.name} Chat Preview`}
    >
      <AIConversation
        open
        presentation="embedded"
        tool={resource.tool}
        sessionId={resource.sessionId}
        messages={resource.messages}
        draft=""
        busy={Boolean(resource.busy)}
        progress={resource.progress}
        interruptible={false}
        activity={resource.activity}
        totalUsage={resource.totalUsage}
        error={resource.error}
        model={resource.model}
        effort={resource.effort}
        readOnly
        readOnlyContext="preview"
      />
    </section>
  );
}

/**
 * Move focus to the dialog's safe default action (Cancel) once it has rendered (HS2-9YGNMX). The dialog
 * renders already open, so its native focusing steps may land on the first focusable area instead:
 * the scrollable running-items List,
 * whose square scroller outline the aside clips. Callers run this after presenting a new dialog.
 */
export function focusProjectCloseDialogDefault(root?: ParentNode): void {
  // Non-DOM callers (unit-tested feature owners) have nothing to focus.
  if (typeof document === 'undefined' || typeof requestAnimationFrame === 'undefined') return;
  requestAnimationFrame(() => {
    const target = (root ?? document).querySelector<HTMLElement & { updateComplete?: Promise<unknown> }>(
      '[data-dialog-component="project-close-dialog"] [data-default-focus]',
    );
    if (!target) return;
    void Promise.resolve(target.updateComplete).then(() => {
      if (target.isConnected) target.focus();
    });
  });
}

export function ProjectCloseDialog({ state }: { state?: ProjectCloseDialogState }) {
  if (!state) return <></>;
  const hasResources = state.resources.length > 0,
    phone = projectCloseDevice.value.handset,
    selected = selectedProjectCloseResource(state.resources, state.selectedKey),
    busy = Boolean(state.operation),
    closingAll = state.operation === 'closing-all',
    closingProject = state.operation === 'closing-project';
  return (
    <div class="project-close-dialog" data-key="project-close-dialog">
      <DialogSurface
        preferredWidth={remify(hasResources ? 832 : 480)}
        viewportGutter={remify(phone ? 8 : 16)}
        maxHeight={phone ? 'viewport' : undefined}
        bodyInset="none"
        label={`Close ${state.projectName}?`}
        rootAttributes={{
          'data-key': 'project-close-dialog',
          ...TERMINALS_TARGETS.projectCloseDialog.attrs,
          'data-project-id': state.projectId,
          'data-has-resources': String(hasResources),
          'data-controlled-open': 'true',
        }}
        // Cancel stands apart on the leading edge; the keep/stop decisions stay grouped on the right (HS2-0DHVGK).
        footer={
          <Row vAlign="middle" gap="xs" wrap flex>
            <Button
              type="button"
              size="compact"
              emphasis="secondary"
              action={TERMINALS_ACTIONS.cancelProjectClose.value}
              disabled={busy}
              rootAttributes={{ 'data-default-focus': '' }}
            >
              Cancel
            </Button>
            <Spacer flex />
            <Button
              type="button"
              size="compact"
              emphasis="secondary"
              action={TERMINALS_ACTIONS.confirmCloseProject.value}
              disabled={busy}
              rootAttributes={{ 'data-project-id': state.projectId }}
            >
              {closingProject ? 'Closing…' : hasResources ? 'Keep Running' : 'Close Project'}
            </Button>
            {hasResources && (
              <Button
                type="button"
                size="compact"
                tone={'danger'}
                action={TERMINALS_ACTIONS.closeAllProjectResources.value}
                disabled={busy}
                rootAttributes={{ 'data-project-id': state.projectId }}
              >
                {closingAll ? 'Stopping…' : 'Stop & Close'}
              </Button>
            )}
          </Row>
        }
      >
        {hasResources && (
          <div id="project-close-dialog-summary">
            <StateBanner
              tone="warning"
              title={projectCloseRunningSummary(state.resources)}
              icon={<LucideIcon size={17.6} icon={CircleAlert} name="circle-alert" />}
            />
          </div>
        )}
        {hasResources ? (
          <div class="project-close-dialog__layout" aria-busy={String(busy)}>
            <aside class="project-close-dialog__resources" aria-label="Running Terminals and AI Chats">
              <List fill scrollable controlInsets="trbl">
                <ListHeader label="Running Items" />
                <nav>
                  <List gap="none">
                    {state.resources.map((resource) => {
                      const key = projectCloseResourceKey(resource),
                        terminal = resource.kind === 'terminal';
                      return (
                        <ListItem
                          action="select-project-close-resource"
                          itemId={key}
                          selected={resource === selected}
                          disabled={busy}
                          icon={
                            <LucideIcon
                              icon={terminal ? SquareTerminal : MessageSquare}
                              name={terminal ? 'square-terminal' : 'message-square'}
                            />
                          }
                          label={resource.name}
                          description={`${terminal ? 'Terminal' : resource.tool} · ${resource.busy ? 'Busy' : 'Running'}`}
                          multiline
                        />
                      );
                    })}
                  </List>
                </nav>
              </List>
            </aside>
            {selected ? (
              <ResourceDetail resource={selected} projectId={state.projectId} />
            ) : (
              <List vAlign="middle" hAlign="center">
                <Text flush>Close this project tab?</Text>
              </List>
            )}
          </div>
        ) : (
          <Text>Close this project tab? You can reopen it later.</Text>
        )}
        <List textInsets="rl" controlInsets="tb" gap="xs">
          {hasResources && (
            <Text flush size="compact" tone="quiet" data-project-close-consequences>
              <strong>Keep running</strong> closes only this tab. Terminals and AI chat tabs return when reopened.
              Received chat history and the latest durable provider session return after an app or server restart.{' '}
              <strong>Stop all</strong> ends every item, then closes the tab.
            </Text>
          )}
          {Boolean(state.error) && (
            <Text flush size="compact" tone="danger" role="alert">
              {state.error}
            </Text>
          )}
        </List>
      </DialogSurface>
    </div>
  );
}
