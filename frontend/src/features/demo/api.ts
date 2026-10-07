import {
  apiGet,
  apiPost,
  listQuery,
  useApiMutation,
  useApiQuery,
} from '../../api/client';
import type {
  DemoSwitchRequest,
  DemoTotpResponse,
  ListResponse,
  LoginResponse,
  MailMessageDto,
  PersonasResponse,
} from '../../api/types';

/**
 * Demo endpoints 404 when PITCAIRN_DEMO_MODE is off. Use `isSuccess`
 * (not the error) to detect demo mode — errors are also produced offline.
 */
export function usePersonas(enabled = true) {
  return useApiQuery<PersonasResponse>(['demo', 'personas'], '/demo/personas', {
    retry: false,
    staleTime: 60_000,
    enabled,
  });
}

export function useDemoSwitch() {
  return useApiMutation<LoginResponse, DemoSwitchRequest>(
    (body) => apiPost<LoginResponse>('/demo/switch', body),
  );
}

export function useDemoMailbox(limit = 50, offset = 0) {
  return useApiQuery<ListResponse<MailMessageDto>>(
    ['demo', 'mailbox', limit, offset],
    `/demo/mailbox${listQuery(limit, offset)}`,
  );
}

/** Current TOTP code for a seeded staff user (demo only). */
export function useDemoTotp(userId: string | undefined, enabled: boolean) {
  return useApiQuery<DemoTotpResponse>(
    ['demo', 'totp', userId],
    `/demo/totp/${userId}`,
    { enabled: enabled && !!userId, retry: false, refetchInterval: 20_000 },
  );
}

/** Probe used by the public landing page (no session needed in demo mode). */
export function fetchPersonas() {
  return apiGet<PersonasResponse>('/demo/personas', { skipAuthRedirect: true });
}
