// Who may use which settings screen. Mirrors the server-side checks in
// backend/src/routes/{admin,templates,resources}.rs (the server stays the
// authority; this only hides controls that would always answer 403).

export const ALL_ROLES = [
  'coordinator',
  'expert',
  'decision_maker',
  'base_manager',
  'finance',
  'admin',
  'provider',
] as const;

/** Application forms: Pitcairn staff (coordinator) and the technical admin. */
export const TEMPLATE_EDITORS = ['admin', 'coordinator'];
/** Resource catalogue: the technical admin and the base manager. */
export const RESOURCE_EDITORS = ['admin', 'base_manager'];
/** Prices (append-only tariffs): resource editors plus finance. */
export const TARIFF_EDITORS = ['admin', 'base_manager', 'finance'];
/** The Users screen: everyone who may grant or revoke at least one role. */
export const ROLE_MANAGERS = ['admin', 'decision_maker', 'coordinator'];

/**
 * Separation of duties: only a decision maker grants `decision_maker` (the
 * technical admin never does); operational roles (expert, provider) belong to
 * the coordinator and the admin; every other staff role to the admin.
 */
function roleGranters(role: string): string[] {
  if (role === 'decision_maker') return ['decision_maker'];
  if (role === 'expert' || role === 'provider') return ['admin', 'coordinator'];
  return ['admin'];
}

export function hasAnyRole(myRoles: readonly string[], roles: readonly string[]): boolean {
  return roles.some((r) => myRoles.includes(r));
}

/** Roles the current user may grant to (and revoke from) other users. */
export function manageableRoles(myRoles: readonly string[]): string[] {
  return ALL_ROLES.filter((role) => hasAnyRole(myRoles, roleGranters(role)));
}
