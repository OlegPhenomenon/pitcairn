import { apiGet, apiPost, useApiMutation, useApiQuery } from '../../api/client';
import type {
  AcceptInvitationResponse,
  LoginRequest,
  LoginResponse,
  MeResponse,
  MfaCodeRequest,
  MfaEnrollResponse,
  RegisterRequest,
} from '../../api/types';

const ME_KEY = ['auth', 'me'] as const;

/**
 * Current session. 401 is expected when logged out and must not trigger the
 * global redirect, so skipAuthRedirect is on; `data` is undefined then.
 */
export function useMe() {
  return useApiQuery<MeResponse>(ME_KEY, '/auth/me', {
    skipAuthRedirect: true,
    retry: false,
    staleTime: 15_000,
  });
}

export function useLogin() {
  return useApiMutation<LoginResponse, LoginRequest>(
    (body) => apiPost<LoginResponse>('/auth/login', body, { skipAuthRedirect: true }),
    { silent: true },
  );
}

export function useRegister() {
  return useApiMutation<LoginResponse, RegisterRequest>(
    (body) => apiPost<LoginResponse>('/auth/register', body, { skipAuthRedirect: true }),
    { silent: true },
  );
}

export function useLogout() {
  return useApiMutation<void, void>(() => apiPost<void>('/auth/logout'), {
    silent: true,
  });
}

export function useMfaVerify() {
  return useApiMutation<MeResponse, MfaCodeRequest>(
    (body) => apiPost<MeResponse>('/auth/mfa/verify', body, { skipAuthRedirect: true }),
    { silent: true },
  );
}

export function useMfaEnroll() {
  return useApiMutation<MfaEnrollResponse, void>(
    () => apiPost<MfaEnrollResponse>('/auth/mfa/enroll', undefined, { skipAuthRedirect: true }),
    { silent: true },
  );
}

export function useMfaEnrollConfirm() {
  return useApiMutation<MeResponse, MfaCodeRequest>(
    (body) =>
      apiPost<MeResponse>('/auth/mfa/enroll/confirm', body, { skipAuthRedirect: true }),
    { silent: true },
  );
}

export function useAcceptInvitation() {
  return useApiMutation<AcceptInvitationResponse, string>(
    (token) => apiPost<AcceptInvitationResponse>(`/invitations/${token}`),
  );
}

export function fetchMe() {
  return apiGet<MeResponse>('/auth/me', { skipAuthRedirect: true });
}
