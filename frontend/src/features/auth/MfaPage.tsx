import { useState, type FormEvent } from 'react';
import { Navigate, useNavigate, useSearchParams } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import { ShieldCheck } from 'lucide-react';

import { fieldError } from '../../api/client';
import { Banner, Button, FormField, Input } from '../../ui';
import { useMe, useMfaVerify } from './api';
import { safeNext } from './guards';
import { AuthShell } from './AuthShell';
import { useDemoTotp } from '../demo/api';
import { PageLoading } from '../../ui';

/** /mfa — verify the 6-digit authenticator code for this session. */
export function MfaPage() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const me = useMe();
  const verify = useMfaVerify();
  const [code, setCode] = useState('');

  const next = safeNext(params.get('next'));
  const demoTotp = useDemoTotp(me.data?.user.id, Boolean(me.data?.demo_mode));

  if (me.isPending) return <PageLoading />;
  if (!me.data) {
    return <Navigate to={`/login?next=${encodeURIComponent('/mfa')}`} replace />;
  }
  if (me.data.mfa_verified) {
    return <Navigate to={next} replace />;
  }
  if (!me.data.user.totp_enrolled) {
    return <Navigate to={`/mfa/enroll?next=${encodeURIComponent(next)}`} replace />;
  }

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const res = await verify.mutateAsync({ code: code.trim() });
      queryClient.setQueryData(['auth', 'me'], res);
      navigate(next, { replace: true });
    } catch {
      // The mutation error appears beside the form.
    }
  };

  const err = verify.error;

  return (
    <AuthShell
      title="Two-factor verification"
      subtitle="Enter the 6-digit code from your authenticator app to continue."
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
        {err && <Banner tone="error">{err.message}</Banner>}
        {demoTotp.data && (
          <Banner tone="demo">
            <span className="inline-flex items-center gap-1">
              <ShieldCheck className="size-4" aria-hidden /> Demo mode: the current code is{' '}
              <code className="font-mono text-base font-bold">{demoTotp.data.code}</code>
            </span>
          </Banner>
        )}
        <FormField label="Verification code" required error={fieldError(err, 'code')}>
          <Input
            name="code"
            inputMode="numeric"
            autoComplete="one-time-code"
            pattern="[0-9]{6}"
            maxLength={6}
            value={code}
            onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
            required
            autoFocus
            className="font-mono tracking-[0.3em]"
          />
        </FormField>
        <Button type="submit" loading={verify.isPending} className="w-full">
          Verify
        </Button>
      </form>
    </AuthShell>
  );
}
