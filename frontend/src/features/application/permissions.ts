import { useProjectContext } from '../projects/projectContext';

export function usePermissions() {
  const { project, me } = useProjectContext();
  const roles = me?.user.roles ?? [];
  const access = project.my_access;
  return {
    canEdit: (access === 'team_editor' || access === 'team_lead') && (project.status === 'draft' || project.status === 'changes_requested'),
    teamEditor: access === 'team_editor' || access === 'team_lead',
    lead: access === 'team_lead',
    coordinator: roles.includes('coordinator'),
    decisionMaker: roles.includes('decision_maker'),
    baseManager: roles.includes('base_manager'),
    expert: roles.includes('expert') && access === 'expert',
    canComment: roles.includes('coordinator') || roles.includes('decision_maker') || roles.includes('base_manager') || (roles.includes('expert') && access === 'expert'),
    staff: access === 'staff' || access === 'expert',
  };
}

