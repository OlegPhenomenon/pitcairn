import { useState } from 'react';
import { useNavigate } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import { Banner } from '../../ui';
import { safeNext } from '../auth/guardUtils';
import { useDemoSwitch, usePersonas } from './api';

const descriptions: Record<string, [string, string]> = {
  anna: ['Researcher — project lead', 'Create an application, reply to staff and submit results.'],
  liam: ['Research team member', 'Help Anna prepare the application and project documents.'],
  priya: ['Research team member', 'Help the team edit the application and share results.'],
  tomasi: ['Research team member', 'Follow the project and its fieldwork plans.'],
  lukas: ['Researcher — project lead', 'Manage a second research project.'],
  maria: ['Pitcairn coordinator', 'Screen applications, coordinate reviews and check results.'],
  james: ['Scientific expert', 'Review assigned research and give your opinion.'],
  helen: ['Permit decision maker', 'Issue permits, refusals and amendments.'],
  sam: ['Base manager', 'Confirm rooms, lab space and equipment.'],
  ruth: ['Finance officer', 'Issue invoices and verify payments.'],
  david: ['Boat provider', 'Respond to requests for boat services.'],
  admin: ['Site admin', 'Manage users, templates and system settings.'],
};

export function PersonaPicker({ next }: { next?: string }) {
  const personas = usePersonas();
  const switchPersona = useDemoSwitch();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [error, setError] = useState('');
  if (!personas.isSuccess) return null;

  async function choose(key: string) {
    setError('');
    try {
      await switchPersona.mutateAsync({ persona_key: key });
      queryClient.clear();
      navigate(safeNext(next), { replace: true });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not open this demo persona.');
    }
  }

  return <section aria-labelledby="persona-heading" className="space-y-4">
    <div><h2 id="persona-heading" className="text-lg font-bold text-navy-900">Choose a demo persona</h2><p className="text-sm text-slate-600">Explore with fictional people and data.</p></div>
    {error && <Banner tone="error">{error}</Banner>}
    <div className="grid gap-3 sm:grid-cols-2">
      {personas.data.personas.map(persona => {
        const [role, action] = descriptions[persona.key] ?? [persona.role?.replaceAll('_', ' ') ?? 'Researcher', 'Explore this person’s workspace.'];
        return <button key={persona.key} type="button" disabled={switchPersona.isPending} onClick={() => void choose(persona.key)} className="rounded-lg border border-slate-300 bg-white p-4 text-left shadow-sm hover:border-teal-700 hover:bg-teal-50 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-teal-700 disabled:opacity-60">
          <span className="block font-semibold text-navy-900">{persona.name}</span><span className="block text-sm font-medium text-teal-800">{role}</span><span className="mt-1 block text-xs text-slate-600">{persona.organisation}</span><span className="mt-2 block text-sm text-slate-700">{action}</span>
        </button>;
      })}
    </div>
  </section>;
}
