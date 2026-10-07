import { useState, type FormEvent } from 'react';
import { Link, useNavigate } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';

import { fieldError } from '../../api/client';
import { Banner, Button, FormField, Input } from '../../ui';
import { useRegister } from './api';
import { AuthShell } from './AuthShell';

export function RegisterPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const register = useRegister();
  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [organisation, setOrganisation] = useState('');
  const [password, setPassword] = useState('');

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await register.mutateAsync({ email, name, organisation, password });
      await queryClient.invalidateQueries();
      navigate('/app', { replace: true });
    } catch {
      // The mutation error appears beside the form.
    }
  };

  const err = register.error;

  return (
    <AuthShell
      title="Create your account"
      subtitle="For researchers applying to work at the Pitcairn Islands Marine Science Base."
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
        {err && !err.fields && <Banner tone="error">{err.message}</Banner>}
        <FormField label="Full name" required error={fieldError(err, 'name')}>
          <Input
            name="name"
            autoComplete="name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </FormField>
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
        <FormField
          label="Organisation"
          error={fieldError(err, 'organisation')}
          help="University, institute or company you represent."
        >
          <Input
            name="organisation"
            autoComplete="organization"
            value={organisation}
            onChange={(e) => setOrganisation(e.target.value)}
          />
        </FormField>
        <FormField
          label="Password"
          required
          error={fieldError(err, 'password')}
          help="At least 8 characters."
        >
          <Input
            name="password"
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
        </FormField>
        <Button type="submit" loading={register.isPending} className="w-full">
          Create account
        </Button>
      </form>
      <p className="mt-4 text-center text-sm text-slate-600">
        Already registered?{' '}
        <Link to="/login" className="font-medium text-teal-700 hover:underline">
          Log in
        </Link>
      </p>
    </AuthShell>
  );
}
