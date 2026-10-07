import { useState } from 'react';
import { Banner, Button, Card, CardBody, FormField, Input, PageHeader, Table } from '../../ui';
import { formatDateTime } from '../../lib/format';
import { useAdminAudit } from './api';

export function AuditPage() {
  const [projectId, setProjectId] = useState('');
  const [filter, setFilter] = useState('');
  const audit = useAdminAudit(filter ? { project_id: filter } : {});
  return <>
    <PageHeader title="Audit log" subtitle="Recorded changes across the hub." />
    <Card><CardBody>
      <form className="mb-4 flex items-end gap-2" onSubmit={(e) => { e.preventDefault(); setFilter(projectId.trim()); }}>
        <FormField label="Project ID" className="max-w-sm flex-1"><Input value={projectId} onChange={(e) => setProjectId(e.target.value)} /></FormField>
        <Button type="submit" variant="secondary">Filter</Button>
      </form>
      {audit.isError && <Banner tone="error">Could not load the audit log.</Banner>}
      <Table caption="Audit events" loading={audit.isPending} rows={audit.data?.items ?? []} rowKey={(event) => event.id} empty={{ title: 'No events found' }} columns={[
        { header: 'When', cell: (event) => formatDateTime(event.at) },
        { header: 'Who', cell: (event) => event.actor_label },
        { header: 'Action', cell: (event) => event.action },
        { header: 'Summary', cell: (event) => event.summary },
        { header: 'Reason', cell: (event) => event.reason ?? '—', hideOnCard: true },
      ]} />
    </CardBody></Card>
  </>;
}
