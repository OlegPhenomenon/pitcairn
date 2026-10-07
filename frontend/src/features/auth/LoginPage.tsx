import { useState, type FormEvent } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';

import { fieldError } from '../../api/client';
import { Button, Banner, FormField, Input } from '../../ui';
import { useLogin } from './api';
import { safeNext } from './guards';
import { AuthShell } from './AuthShell';
import { usePersonas } from '../demo/api';

export function LoginPage() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const login = useLogin();
  const personas = usePersonas();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');

  const next = safeNext(params.get('next'));

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const res = await login.mutateAsync({ email, password });
      await queryClient.invalidateQueries();
      if (res.mfa_enrollment_required) {
        navigate(`/mfa/enroll?next=${encodeURIComponent(next)}`, { replace: true });
      } else if (res.mfa_required) {
        navigate(`/mfa?next=${encodeURIComponent(next)}`, { replace: true });
      } else {
        navigate(next, { replace: true });
      }
    } catch {
      // The mutation error appears beside the form.
    }
  };

  const err = login.error;

  return (
    <AuthShell
      title="Log in"
      subtitle="Sign in with your email and password. Staff and reviewers will also need their authenticator code."
    >
      {personas.isSuccess && (
        <Banner tone="demo" className="mb-4">
          Demo accounts: pick any persona with password{' '}
          <code className="font-mono font-semibold">demo-pass-2026</code> — or use the
          persona switcher after logging in as anyone.
        </Banner>
      )}
      <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
        {err && !err.fields && (
          <Banner tone="error">{err.message}</Banner>
        )}
        <FormField label="Email" required error={fieldError(err, 'email')}>
          <Input
            name="email"
            type="email"
            autoComplete="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
        </FormField>
        <FormField label="Password" required error={fieldError(err, 'password')}>
          <Input
            name="password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </FormField>
        <Button type="submit" loading={login.isPending} className="w-full">
          Log in
        </Button>
      </form>
      <p className="mt-4 text-center text-sm text-slate-600">
        No account yet?{' '}
        <Link to="/register" className="font-medium text-teal-700 hover:underline">
          Register
        </Link>
      </p>
    </AuthShell>
  );
}
