import { Banner, Card, CardBody, PageHeader, StatusBadge, Table } from '../../ui';
import { formatDateTime } from '../../lib/format';
import { useDemoMailbox } from './api';

export function MailboxPage() {
  const mail = useDemoMailbox();
  return <>
    <PageHeader title="Demo mailbox" subtitle="Fictional outbound messages created by hub actions." />
    <Card><CardBody>
      {mail.isError && <Banner tone="error">Could not load the demo mailbox.</Banner>}
      <Table caption="Demo mail" loading={mail.isPending} rows={mail.data?.items ?? []} rowKey={(item) => String(item.id)} empty={{ title: 'No messages yet' }} columns={[
        { header: 'To', cell: (item) => item.to_email },
        { header: 'Subject and body', cell: (item) => <div><strong>{item.subject}</strong><p className="whitespace-pre-wrap text-sm text-slate-600">{item.body_text}</p></div> },
        { header: 'Status', cell: (item) => <div><StatusBadge status={item.status} />{item.error && <p className="text-sm text-red-700">{item.error}</p>}</div> },
        { header: 'Created', cell: (item) => formatDateTime(item.created_at), hideOnCard: true },
      ]} />
    </CardBody></Card>
  </>;
}
