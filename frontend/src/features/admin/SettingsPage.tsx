import { useState } from 'react';
import { Banner, Button, Card, CardBody, Checkbox, FormField, Input, PageHeader } from '../../ui';
import { useAdminSettings, usePutSettings } from './api';

export function SettingsPage() {
  const settings = useAdminSettings();
  const save = usePutSettings();
  const [draft, setDraft] = useState(settings.data);
  const value = draft ?? settings.data;
  if (settings.isPending) return <p>Loading settings…</p>;
  if (!value) return <Banner tone="error">Could not load settings.</Banner>;
  return <>
    <PageHeader title="Settings" subtitle="Install-wide behaviour and public information." />
    <Card><CardBody>
      <form className="flex max-w-xl flex-col gap-5" onSubmit={async (event) => {
        event.preventDefault();
        try { await save.mutateAsync(value); } catch { /* Error is shown above. */ }
      }}>
        {save.error && <Banner tone="error">{save.error.message}</Banner>}
        {save.isSuccess && <Banner tone="info">Settings saved.</Banner>}
        <FormField label="Organisation name"><Input value={value.organisation_name} onChange={(e) => setDraft({ ...value, organisation_name: e.target.value })} /></FormField>
        <FormField label="Reference prefix"><Input value={value.reference_prefix} onChange={(e) => setDraft({ ...value, reference_prefix: e.target.value })} /></FormField>
        <Checkbox label="Public catalog enabled" checked={value.public_catalog_enabled} onChange={(e) => setDraft({ ...value, public_catalog_enabled: e.target.checked })} />
        <div><Checkbox label="Mail enabled" checked={value.mail_enabled} onChange={(e) => setDraft({ ...value, mail_enabled: e.target.checked })} />
          <p className="ml-6 text-sm text-slate-600">Turn mail off to simulate mail outage. In-app notifications and other actions still work; failed delivery jobs appear on the Jobs page.</p>
        </div>
        <Button type="submit" loading={save.isPending}>Save settings</Button>
      </form>
    </CardBody></Card>
  </>;
}
