import { useNavigate } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import { UserRound } from 'lucide-react';

import { Dropdown, MenuItem, useToast } from '../../ui';
import { useDemoSwitch, usePersonas } from './api';

/**
 * Header persona switcher (demo mode only). Switching creates a new session
 * server-side; we then clear the query cache and land on /app.
 */
export function PersonaSwitcher({ demoMode }: { demoMode: boolean }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const toast = useToast();
  const personas = usePersonas(demoMode);
  const switchPersona = useDemoSwitch();

  if (!demoMode || !personas.data || personas.data.personas.length === 0) return null;

  const onSwitch = async (key: string) => {
    try {
      await switchPersona.mutateAsync({ persona_key: key });
      queryClient.clear();
      navigate('/app');
    } catch {
      toast.error('Could not switch persona');
    }
  };

  return (
    <Dropdown
      menuLabel="Switch demo persona"
      align="right"
      trigger={(props) => (
        <button
          type="button"
          {...props}
          className="inline-flex items-center gap-1.5 rounded-md bg-teal-700/80 px-2.5 py-1.5 text-xs font-semibold text-white hover:bg-teal-600"
          title="Switch demo persona"
        >
          <UserRound className="size-4" aria-hidden />
          <span className="hidden sm:inline">Switch persona</span>
        </button>
      )}
    >
      {(close) => (
        <div className="w-80 max-w-[85vw]" role="menu">
          {personas.data.personas.map((p) => (
            <MenuItem
              key={p.key}
              onClick={() => {
                close();
                void onSwitch(p.key);
              }}
              className="flex-col items-start gap-0.5"
            >
              <span className="flex w-full items-center justify-between gap-2">
                <span className="font-medium">{p.name}</span>
                {p.role && (
                  <span className="rounded bg-teal-50 px-1.5 py-0.5 text-[11px] font-semibold text-teal-800">
                    {p.role.replaceAll('_', ' ')}
                  </span>
                )}
              </span>
              <span className="text-xs text-slate-500">{p.organisation}</span>
            </MenuItem>
          ))}
        </div>
      )}
    </Dropdown>
  );
}
