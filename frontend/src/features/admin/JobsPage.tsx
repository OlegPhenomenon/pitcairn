import { useState } from 'react';
import { Banner, Button, Card, CardBody, PageHeader, Select, StatusBadge, Table } from '../../ui';
import { formatDateTime, toNum } from '../../lib/format';
import { useAdminJobs, useRetryJob } from './api';

export function JobsPage() {
  const [status, setStatus] = useState('failed');
  const jobs = useAdminJobs(status);
  const retry = useRetryJob();
  return <>
    <PageHeader title="Delivery jobs" subtitle="Failed and dead background jobs, including mail delivery." />
    <Card><CardBody>
      <label className="mb-4 block max-w-xs text-sm font-medium">Status
        <Select value={status} onChange={(e) => setStatus(e.target.value)}><option value="failed">Failed</option><option value="dead">Dead</option></Select>
      </label>
      {jobs.isError && <Banner tone="error">Could not load jobs.</Banner>}
      {retry.error && <Banner tone="error">{retry.error.message}</Banner>}
      <Table caption="Delivery jobs" loading={jobs.isPending} rows={jobs.data?.items ?? []} rowKey={(job) => job.id} empty={{ title: `No ${status} jobs` }} columns={[
        { header: 'Kind', cell: (job) => job.kind },
        { header: 'Status', cell: (job) => <StatusBadge status={job.status} /> },
        { header: 'Attempts', cell: (job) => `${toNum(job.attempts)} / ${toNum(job.max_attempts)}` },
        { header: 'Last error', cell: (job) => <span className="break-words text-red-700">{job.last_error ?? '—'}</span> },
        { header: 'Created', cell: (job) => formatDateTime(job.created_at), hideOnCard: true },
        { header: 'Action', cell: (job) => <Button size="sm" variant="secondary" loading={retry.isPending && retry.variables === job.id} onClick={() => void retry.mutateAsync(job.id).catch(() => undefined)}>Retry</Button> },
      ]} />
    </CardBody></Card>
  </>;
}
