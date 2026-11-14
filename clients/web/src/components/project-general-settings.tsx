import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';

import type { ProjectType } from '../api';

export interface ProjectGeneralSettingsProps {
  /** The stored type; absent while it is read for the first time. */
  projectType?: ProjectType;
  /** Projects in the target declaration, kept while the type is Software. */
  activeProjects?: number;
  loading?: boolean;
  saving?: boolean;
  message?: string;
}

const HINTS: Readonly<Record<ProjectType, string>> = {
  software: 'A project with its own code and tickets.',
  orchestration: 'This project plans work across other projects. Choose them in Orchestration settings.',
};

/**
 * Project Settings → General (HS2-W27N1D): the project-scoped settings that come first, starting with
 * Project Type. The type is the checked-in `project_type` setting; changing it goes through the same
 * workflow as `hotsheet-cli orchestration project-type --set` and MCP `project_type_set`.
 */
export function ProjectGeneralSettings({
  projectType,
  activeProjects = 0,
  loading = false,
  saving = false,
  message = '',
}: ProjectGeneralSettingsProps) {
  const kind = projectType ?? 'software',
    kept =
      kind === 'software' && activeProjects > 0
        ? `${activeProjects === 1 ? '1 project stays' : `${String(activeProjects)} projects stay`} in this project's orchestration settings for a switch back to Orchestration.`
        : '';
  return (
    <section data-component="project-general-settings" data-project-type={projectType ?? ''}>
      <List gap="xs">
        <Select
          name="project-type"
          label="Project Type"
          value={kind}
          placeholder={loading && !projectType}
          disabled={saving || (loading && !projectType)}
          hint={kept ? `${HINTS[kind]} ${kept}` : HINTS[kind]}
          choices={[
            { value: 'software', label: 'Software' },
            { value: 'orchestration', label: 'Orchestration' },
          ]}
        />
        {message ? (
          <Text tone="danger" size="compact" role="alert" flush>
            {message}
          </Text>
        ) : null}
      </List>
    </section>
  );
}
