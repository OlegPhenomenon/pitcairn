import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationOptions,
  type UseQueryOptions,
} from '@tanstack/react-query';
import { useEffect } from 'react';

import { useToast } from '../ui/toastContext';

const API_BASE = '/api/v1';
const CSRF_HEADER = 'X-Pitcairn-Csrf';

/**
 * Error shape of §5: `{ "error": { "code", "message", "fields"? } }`.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly fields: Record<string, string> | undefined;

  constructor(
    status: number,
    code: string,
    message: string,
    fields?: Record<string, string>,
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.fields = fields;
  }

  fieldError(name: string): string | undefined {
    return this.fields?.[name];
  }
}

interface ErrorBody {
  error?: {
    code?: unknown;
    message?: unknown;
    fields?: unknown;
  };
}

/** Parse a §5 error body into an ApiError. Exported for tests. */
export function parseErrorBody(status: number, body: unknown): ApiError {
  const err = (body as ErrorBody | null)?.error;
  if (err && typeof err === 'object') {
    const code = typeof err.code === 'string' ? err.code : `http_${status}`;
    const message =
      typeof err.message === 'string' ? err.message : `Request failed (${status})`;
    const fields =
      err.fields && typeof err.fields === 'object'
        ? (err.fields as Record<string, string>)
        : undefined;
    return new ApiError(status, code, message, fields);
  }
  return new ApiError(status, `http_${status}`, `Request failed (${status})`);
}

// ---------------------------------------------------------------------------
// Redirects: 401 → /login?next=…, 403 mfa_required → /mfa?next=…
// ---------------------------------------------------------------------------

type RedirectFn = (path: string) => void;

function defaultRedirect(path: string) {
  try {
    window.location.assign(path);
  } catch {
    // jsdom / non-navigable environments: nothing sensible to do.
  }
}

let redirectFn: RedirectFn = defaultRedirect;

/** The app registers its router navigate here so session redirects use the SPA router. */
export function setAuthRedirect(fn: RedirectFn) {
  redirectFn = fn;
}

/** Routes where a session-expired redirect must never trigger (they are public or auth pages). */
const PUBLIC_PREFIXES = ['/login', '/demo', '/register', '/mfa', '/invite', '/catalog'];

function currentPath(): string {
  try {
    return window.location.pathname;
  } catch {
    return '/';
  }
}

function handleAuthRedirect(error: ApiError) {
  const path = currentPath();
  const onPublicPage =
    path === '/' || PUBLIC_PREFIXES.some((p) => path.startsWith(p));
  if (error.status === 401 && !onPublicPage) {
    redirectFn(`/login?next=${encodeURIComponent(path)}`);
  }
  if (error.status === 403 && error.code === 'mfa_required' && !path.startsWith('/mfa')) {
    redirectFn(`/mfa?next=${encodeURIComponent(path)}`);
  }
}

// ---------------------------------------------------------------------------
// Fetch wrapper
// ---------------------------------------------------------------------------

export interface ApiRequestOptions {
  method?: string;
  /** JSON body (encoded with a bigint→number replacer; ts-rs maps u64→bigint). */
  body?: unknown;
  /** Raw body for non-JSON payloads (chunk PUT). Content-Type must be set by caller. */
  rawBody?: BodyInit;
  headers?: Record<string, string>;
  /** Don't auto-redirect on 401 / mfa_required (used by useMe and auth pages). */
  skipAuthRedirect?: boolean;
  signal?: AbortSignal;
}

function serializeBody(body: unknown): string {
  return JSON.stringify(body, (_key, value: unknown) =>
    typeof value === 'bigint' ? Number(value) : value,
  );
}

export async function api<T = unknown>(
  path: string,
  opts: ApiRequestOptions = {},
): Promise<T> {
  const method = opts.method ?? (opts.body !== undefined ? 'POST' : 'GET');
  const headers: Record<string, string> = { ...opts.headers };
  if (method !== 'GET' && method !== 'HEAD') {
    headers[CSRF_HEADER] = '1';
  }
  let body: BodyInit | undefined;
  if (opts.rawBody !== undefined) {
    body = opts.rawBody;
  } else if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = serializeBody(opts.body);
  }

  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      method,
      headers,
      body,
      credentials: 'include',
      signal: opts.signal,
    });
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    throw new ApiError(0, 'network_error', 'Network error — check your connection.');
  }

  if (res.status === 204) return undefined as T;

  const text = await res.text();
  let parsed: unknown = undefined;
  if (text.length > 0) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = undefined;
    }
  }

  if (!res.ok) {
    const error = parseErrorBody(res.status, parsed);
    if (!opts.skipAuthRedirect) handleAuthRedirect(error);
    throw error;
  }
  return parsed as T;
}

export const apiGet = <T>(path: string, opts?: ApiRequestOptions) =>
  api<T>(path, { ...opts, method: 'GET' });
export const apiPost = <T>(path: string, body?: unknown, opts?: ApiRequestOptions) =>
  api<T>(path, { ...opts, method: 'POST', body });
export const apiPatch = <T>(path: string, body: unknown, opts?: ApiRequestOptions) =>
  api<T>(path, { ...opts, method: 'PATCH', body });
export const apiPut = <T>(path: string, body: unknown, opts?: ApiRequestOptions) =>
  api<T>(path, { ...opts, method: 'PUT', body });
export const apiDelete = <T>(path: string, opts?: ApiRequestOptions) =>
  api<T>(path, { ...opts, method: 'DELETE' });

// ---------------------------------------------------------------------------
// React Query helpers
// ---------------------------------------------------------------------------

function retryPolicy(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiError) {
    if (error.status === 0) return failureCount < 2;
    if (error.status === 429) return failureCount < 2;
    if (error.status >= 500) return failureCount < 2;
    return false;
  }
  return failureCount < 2;
}

type ApiQueryOptions<T> = Omit<
  UseQueryOptions<T, ApiError>,
  'queryKey' | 'queryFn'
> & { skipAuthRedirect?: boolean };

export function useApiQuery<T>(
  queryKey: readonly unknown[],
  path: string,
  opts?: ApiQueryOptions<T>,
) {
  const { skipAuthRedirect, ...rest } = opts ?? {};
  const toast = useToast();
  const query = useQuery<T, ApiError>({
    queryKey,
    queryFn: ({ signal }) => api<T>(path, { signal, skipAuthRedirect }),
    retry: retryPolicy,
    ...rest,
  });
  useEffect(() => {
    if (query.error && (query.error.status === 0 || query.error.status >= 500)) {
      toast.error('Could not load data', query.error.message);
    }
  }, [query.error, toast]);
  return query;
}

type ApiMutationOptions<TRes, TVars> = {
  invalidate?: readonly (readonly unknown[])[];
  /** Toast title on unexpected errors; default shows the server message. */
  errorTitle?: string;
  /** Suppress the error toast (e.g. when the form shows the message inline). */
  silent?: boolean;
} & Omit<
  UseMutationOptions<TRes, ApiError, TVars>,
  'mutationFn' | 'onError' | 'onSuccess'
> & {
    onSuccess?: (data: TRes, vars: TVars) => void;
    onError?: (error: ApiError, vars: TVars) => void;
  };

/**
 * Mutation wrapper: toasts unexpected errors (validation errors with
 * `error.fields` are passed back to the form instead of toasted).
 */
export function useApiMutation<TRes = unknown, TVars = void>(
  mutationFn: (vars: TVars) => Promise<TRes>,
  opts?: ApiMutationOptions<TRes, TVars>,
) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const { invalidate, errorTitle, silent, onSuccess, onError, ...rest } = opts ?? {};

  return useMutation<TRes, ApiError, TVars>({
    mutationFn,
    onSuccess: (data, vars, result, ctx) => {
      for (const key of invalidate ?? []) {
        void queryClient.invalidateQueries({ queryKey: key });
      }
      onSuccess?.(data, vars);
      void result;
      void ctx;
    },
    onError: (error, vars, _result, _ctx) => {
      const hasFieldErrors =
        error instanceof ApiError &&
        error.fields &&
        Object.keys(error.fields).length > 0;
      if (!silent && !hasFieldErrors) {
        toast.push({
          tone: 'error',
          title: errorTitle ?? 'Something went wrong',
          body: error.message,
        });
      }
      onError?.(error, vars);
    },
    ...rest,
  });
}

/** Convenience for forms: field-level message from a mutation error. */
export function fieldError(error: unknown, name: string): string | undefined {
  return error instanceof ApiError ? error.fieldError(name) : undefined;
}

/** Build the ?limit&offset query string shared by all list endpoints. */
export function listQuery(limit?: number, offset?: number, extra?: Record<string, string | number | undefined>): string {
  const params = new URLSearchParams();
  if (limit !== undefined) params.set('limit', String(limit));
  if (offset !== undefined) params.set('offset', String(offset));
  for (const [k, v] of Object.entries(extra ?? {})) {
    if (v !== undefined && v !== '') params.set(k, String(v));
  }
  const s = params.toString();
  return s ? `?${s}` : '';
}
