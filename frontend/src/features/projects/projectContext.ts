import { useOutletContext } from 'react-router';
import type { MeResponse, ProjectWorkspaceDto } from '../../api/types';

export interface ProjectContext {
  workspace: ProjectWorkspaceDto;
  project: ProjectWorkspaceDto['project'];
  me: MeResponse | undefined;
}

export function useProjectContext(): ProjectContext {
  return useOutletContext<ProjectContext>();
}
