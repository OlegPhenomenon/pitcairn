import { useEffect, useState, type FormEvent } from 'react';
import { Navigate, useNavigate, useSearchParams } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import QRCode from 'qrcode';

import { ApiError, fieldError } from '../../api/client';
import { Banner, Button, FormField, Input, PageLoading } from '../../ui';
import { useMe, useMfaEnroll, useMfaEnrollConfirm } from './api';
import { safeNext } from './guards';
import { AuthShell } from './AuthShell';

/**
 * /mfa/enroll — start enrollment (POST /auth/mfa/enroll returns an otpauth
 * URI + secret), show a QR and the manual secret, then confirm with a code.
 */
export function MfaEnrollPage() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const me = useMe();
  const enroll = useMfaEnroll();
  const confirm = useMfaEnrollConfirm();
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [code, setCode] = useState('');
  const [enrollError, setEnrollError] = useState<ApiError | null>(null);

  const next = safeNext(params.get('next'));

  // Start enrollment on mount.
  useEffect(() => {
    if (!me.data || me.data.user.totp_enrolled) return;
    let cancelled = false;
    enroll
      .mutateAsync()
      .then(async (res) => {
        if (cancelled) return;
        const url = await QRCode.toDataURL(res.otpauth_url, { margin: 1, width: 200 });
        setQrDataUrl(url);
      })
      .catch((e) => {
        if (!cancelled) setEnrollError(e instanceof ApiError ? e : null);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me.data?.user.id]);

  if (me.isPending) return <PageLoading />;
  if (!me.data) {
    return <Navigate to={`/login?next=${encodeURIComponent('/mfa/enroll')}`} replace />;
  }
  if (me.data.mfa_verified && me.data.user.totp_enrolled) {
    return <Navigate to={next} replace />;
  }

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const res = await confirm.mutateAsync({ code: code.trim() });
      queryClient.setQueryData(['auth', 'me'], res);
      navigate(next, { replace: true });
    } catch {
      // The mutation error appears beside the form.
    }
  };

  const err = confirm.error;
  const enrollment = enroll.data;
  const alreadyEnrolled =
    enrollError?.code === 'already_enrolled' ||
    (me.data.user.totp_enrolled && !me.data.mfa_verified);

  return (
    <AuthShell
      title="Set up two-factor authentication"
      subtitle="Staff and reviewer accounts must protect sign-in with an authenticator app (e.g. 1Password, Authy, Google Authenticator)."
    >
      {alreadyEnrolled ? (
        <div className="flex flex-col gap-4">
          <Banner tone="info">
            You already have an authenticator set up. Continue to verification.
          </Banner>
          <Button onClick={() => navigate(`/mfa?next=${encodeURIComponent(next)}`)}>
            Continue to verification
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          {enrollError && !alreadyEnrolled && (
            <Banner tone="error">{enrollError.message}</Banner>
          )}
          <ol className="list-decimal space-y-4 pl-5 text-sm text-slate-700">
            <li>
              <p className="font-medium">Scan this QR code with your authenticator app.</p>
              <div className="mt-2 flex justify-center rounded-md border border-slate-200 bg-white p-3">
                {qrDataUrl ? (
                  <img src={qrDataUrl} alt="QR code for authenticator setup" width={200} height={200} />
                ) : (
                  <div className="flex h-[200px] w-[200px] items-center justify-center text-sm text-slate-500">
                    {enroll.isPending ? 'Generating…' : 'QR unavailable'}
                  </div>
                )}
              </div>
            </li>
            <li>
              <p className="font-medium">Or enter this secret manually:</p>
              <code className="mt-1 block rounded bg-slate-100 px-3 py-2 font-mono text-sm break-all select-all">
                {enrollment?.secret ?? '…'}
              </code>
            </li>
            <li>
              <form onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
                <FormField
                  label="Enter the 6-digit code shown in the app"
                  required
                  error={fieldError(err, 'code') || (err && !err.fields ? err.message : undefined)}
                >
                  <Input
                    name="code"
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    pattern="[0-9]{6}"
                    maxLength={6}
                    value={code}
                    onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                    required
                    className="font-mono tracking-[0.3em]"
                  />
                </FormField>
                <Button
                  type="submit"
                  loading={confirm.isPending}
                  disabled={!enrollment}
                  className="w-full"
                >
                  Confirm and finish setup
                </Button>
              </form>
            </li>
          </ol>
        </div>
      )}
    </AuthShell>
  );
}
