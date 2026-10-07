import { Link, useSearchParams } from 'react-router';
import { AuthShell } from '../auth/AuthShell';
import { PersonaPicker } from './PersonaPicker';
import { usePersonas } from './api';

export function DemoPage() {
  const [params] = useSearchParams();
  const personas = usePersonas();
  return <AuthShell wide title="Try the demo" subtitle="Choose someone to explore the Research Hub.">
    {personas.isPending && <p>Loading demo personas…</p>}
    {personas.isError && <p className="text-sm text-slate-700">The demo is unavailable on this installation. <Link to="/login" className="text-teal-700 underline">Log in</Link></p>}
    <PersonaPicker next={params.get('next') ?? undefined} />
    <p className="mt-5 text-sm"><Link to="/demo/story" className="font-medium text-teal-800 underline">Open the story guide</Link> to follow the nine-step walkthrough.</p>
  </AuthShell>;
}
