import type { MeResponse } from '../../api/types';

export const STAFF_ROLES = [
  'coordinator',
  'decision_maker',
  'base_manager',
  'finance',
  'admin',
] as const;

export function isStaffOrExpert(me: MeResponse | undefined): boolean {
  if (!me) return false;
  return me.user.roles.some(
    (r) => (STAFF_ROLES as readonly string[]).includes(r) || r === 'expert',
  );
}

/** Staff/expert sessions must be MFA-verified before touching the app. */
export function needsMfa(me: MeResponse | undefined): boolean {
  return isStaffOrExpert(me) && !me?.mfa_verified;
}

export function safeNext(raw: string | null | undefined, fallback = '/app'): string {
  if (!raw) return fallback;
  // Only allow same-origin absolute paths.
  if (!raw.startsWith('/') || raw.startsWith('//')) return fallback;
  return raw;
}

