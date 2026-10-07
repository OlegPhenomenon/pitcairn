import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';

import { fieldError } from '../../api/client';
import { formatDate, titleize } from '../../lib/format';
import {
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  Dialog,
  FormField,
  Input,
  KeyValue,
  Select,
  StatusBadge,
  useToast,
} from '../../ui';
import { DocumentSlot } from '../upload/DocumentSlot';
import { FileUpload } from '../upload/FileUpload';
import { projectDocsKey, useCreateDocument, useProjectDocuments } from './api';
import { useProjectContext } from './ProjectLayout';
import { ProjectTools } from '../results/ProjectTools';

const DOC_CATEGORIES = ['application', 'personal', 'decision', 'result', 'other'];

function canUploadDocs(access: string, roles: string[] | undefined): boolean {
  return (
    access === 'team_editor' ||
    access === 'team_lead' ||
    (access === 'staff' && Boolean(roles?.includes('coordinator')))
  );
}

export function OverviewTab() {
  const { project, me } = useProjectContext();
  const docs = useProjectDocuments(project.id);
  const canUpload = canUploadDocs(project.my_access, me?.user.roles);
  const demo = Boolean(me?.demo_mode);
  const [addOpen, setAddOpen] = useState(false);

  return (
    <div className="grid gap-5 lg:grid-cols-3">
      <Card className="lg:col-span-2">
        <CardHeader title="Project" />
        <CardBody>
          <KeyValue
            items={[
              { key: 'Reference', value: project.reference ?? 'Assigned on submit' },
              { key: 'Status', value: <StatusBadge status={project.status} label={project.status === 'approved' ? 'Permit approved — see decision conditions' : undefined} /> },
              { key: 'Organisation', value: project.organisation || '—' },
              {
                key: 'Your access',
                value: titleize(project.my_access.replaceAll('_', ' ')),
              },
              { key: 'Start date', value: formatDate(project.start_date) },
              { key: 'End date', value: formatDate(project.end_date) },
              { key: 'Created', value: formatDate(project.created_at) },
            ]}
          />
          {project.summary && (
            <p className="mt-4 border-t border-slate-100 pt-3 text-sm text-slate-700">
              {project.summary}
            </p>
          )}
          {project.keywords && (
            <p className="mt-2 text-xs text-slate-500">Keywords: {project.keywords}</p>
          )}
          <ProjectTools />
        </CardBody>
      </Card>

      <div className="lg:col-span-1">
        <Card>
          <CardHeader
            title="Documents"
            actions={
              canUpload && (
                <Button
                  size="sm"
                  variant="secondary"
                  icon={<Plus className="size-4" />}
                  onClick={() => setAddOpen(true)}
                >
                  Add
                </Button>
              )
            }
          />
          <CardBody className="flex flex-col gap-3">
            {docs.isPending && <p className="text-sm text-slate-500">Loading…</p>}
            {docs.isError && (
              <Banner tone="error">Could not load documents.</Banner>
            )}
            {docs.data && docs.data.items.length === 0 && (
              <p className="text-sm text-slate-600">
                No documents yet. Upload the files the application needs — a field safety
                plan, insurance certificate, CVs and permits.
              </p>
            )}
            {docs.data?.items.map((doc) => (
              <DocumentSlot
                key={doc.id}
                projectId={project.id}
                document={doc}
                title={doc.title}
                slotKey={doc.slot_key}
                category={doc.category}
                canUpload={canUpload}
                demoMode={demo}
              />
            ))}
          </CardBody>
        </Card>
      </div>

      <AddDocumentDialog
        open={addOpen}
        onClose={() => setAddOpen(false)}
        projectId={project.id}
        demoMode={demo}
      />
    </div>
  );
}

function AddDocumentDialog({
  open,
  onClose,
  projectId,
  demoMode,
}: {
  open: boolean;
  onClose: () => void;
  projectId: string;
  demoMode: boolean;
}) {
  const create = useCreateDocument(projectId);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [title, setTitle] = useState('');
  const [category, setCategory] = useState('application');
  const [slotKey, setSlotKey] = useState('');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);

  const onFileReady = async (fileId: string) => {
    setBusy(true);
    try {
      await create.mutateAsync({
        slot_key: slotKey.trim() || null,
        title,
        category,
        file_id: fileId,
        note: note || null,
      });
      await queryClient.invalidateQueries({ queryKey: projectDocsKey(projectId) });
      toast.success('Document added');
      onClose();
      setTitle('');
      setSlotKey('');
      setNote('');
    } catch {
      // field errors surface through `create.error`; other errors toast via the hook
    } finally {
      setBusy(false);
    }
  };

  const err = create.error;

  return (
    <Dialog open={open} onClose={() => !busy && onClose()} title="Add a document">
      <div className="flex flex-col gap-4">
        {err && !err.fields && <Banner tone="error">{err.message}</Banner>}
        <FormField label="Title" required error={fieldError(err, 'title')}>
          <Input value={title} onChange={(e) => setTitle(e.target.value)} required />
        </FormField>
        <FormField label="Category" required error={fieldError(err, 'category')}
          help="Personal documents are visible only to the team editors and the coordinator.">
          <Select value={category} onChange={(e) => setCategory(e.target.value)}>
            {DOC_CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {titleize(c)}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField
          label="Slot"
          hint="(optional)"
          help="Template slot key such as safety_plan, insurance, cvs or permits."
        >
          <Input value={slotKey} onChange={(e) => setSlotKey(e.target.value)} />
        </FormField>
        <FormField label="Note" hint="(optional)">
          <Input value={note} onChange={(e) => setNote(e.target.value)} />
        </FormField>
        <FileUpload
          demoMode={demoMode}
          disabled={!title.trim() || busy}
          onComplete={onFileReady}
        />
      </div>
    </Dialog>
  );
}
