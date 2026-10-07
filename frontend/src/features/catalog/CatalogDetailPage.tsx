import { Link, useParams } from 'react-router';
import { Anchor, Download } from 'lucide-react';
import { useApiQuery } from '../../api/client';
import type { PublicProjectDto } from '../../api/types';
import { Card, CardBody, EmptyState } from '../../ui';
import { formatBytes } from '../../lib/format';

export function CatalogDetailPage() {
  const { reference } = useParams();
  const project = useApiQuery<PublicProjectDto>(['public', 'projects', reference], `/public/projects/${encodeURIComponent(reference ?? '')}`, { enabled: !!reference, retry: false });
  return <div className="min-h-dvh bg-sand-50">
    <header className="bg-navy-900 px-4 py-3 text-white"><Link to="/" className="mx-auto flex max-w-6xl items-center gap-2 font-semibold"><Anchor className="size-5 text-teal-300" />Pitcairn Research Hub</Link></header>
    <main id="main" className="mx-auto max-w-4xl px-4 py-8">
      <Link to="/catalog" className="text-sm text-teal-700 underline">← Open catalog</Link>
      {project.isPending ? <p className="mt-6">Loading project…</p> : !project.data ? <EmptyState title="Project not found" /> : <>
        <h1 className="mt-5 text-2xl font-bold text-navy-900">{project.data.title}</h1>
        <p className="text-sm text-slate-600">{project.data.reference} · {project.data.organisation}</p>
        <p className="mt-4 text-slate-700">{project.data.summary}</p>
        <h2 className="mt-8 mb-3 text-lg font-semibold">Published results</h2>
        {project.data.deliverables.length === 0 && <p className="text-slate-600">No results published yet.</p>}
        <div className="grid gap-3">{project.data.deliverables.map((item) => <Card key={item.id}><CardBody>
          <h3 className="font-semibold">{item.title}</h3><p className="mt-1 text-sm text-slate-600">{item.description}</p>
          {item.files_available_from && <p className="mt-2 text-sm text-amber-800">Files available from {item.files_available_from}</p>}
          {item.files.map((file) => <a key={file.document_version_id} className="mt-2 flex items-center gap-2 text-sm text-teal-700 underline" href={`/api/v1/public/files/${file.document_version_id}/download`}><Download className="size-4" />{file.title} ({formatBytes(Number(file.size))})</a>)}
        </CardBody></Card>)}</div>
      </>}
    </main>
  </div>;
}
