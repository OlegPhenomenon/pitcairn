import { useState, type FormEvent } from 'react';
import { Download, FileText, UploadCloud } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';

import type { DocumentDto, DocumentVersionDto } from '../../api/types';
import { formatBytes, formatDateTime, shortId, toNum, titleize } from '../../lib/format';
import { Badge, Button, Dialog, FormField, Input, StatusBadge, useToast } from '../../ui';
import { useMe } from '../auth/api';
import {
  documentVersionDownloadUrl,
  useAddDocumentVersion,
  useCreateDocument,
} from '../projects/api';
import { FileUpload } from './FileUpload';

export interface DocumentSlotProps {
  projectId: string;
  /** Existing document occupying this slot, if any. */
  document?: DocumentDto | null;
  /** Full version list when an area supplies one; the current list API supplies latest only. */
  versions?: DocumentVersionDto[];
  /** Slot key from the template's required_documents (e.g. `safety_plan`). */
  slotKey?: string | null;
  /** Display label for the slot/document. */
  title: string;
  category?: string;
  help?: string;
  canUpload?: boolean;
  demoMode?: boolean;
}

/**
 * A project document slot: shows available versions
 * (number, uploader, date, download link), and uploads a new version
 * (or creates the document) via the chunked uploader.
 */
export function DocumentSlot({
  projectId,
  document,
  versions,
  slotKey,
  title,
  category = 'application',
  help,
  canUpload = true,
  demoMode = false,
}: DocumentSlotProps) {
  const me = useMe();
  const toast = useToast();
  const queryClient = useQueryClient();
  const createDoc = useCreateDocument(projectId);
  const addVersion = useAddDocumentVersion(projectId);
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState('');
  const [docTitle, setDocTitle] = useState(title);
  const [attaching, setAttaching] = useState(false);

  const version = document?.latest_version ?? null;
  const visibleVersions = versions ?? (version ? [version] : []);
  const uploaderName = (id: string) =>
    id === me.data?.user.id ? 'you' : `user ${shortId(id)}`;

  const onFileReady = async (fileId: string) => {
    setAttaching(true);
    try {
      if (document) {
        await addVersion.mutateAsync({ documentId: document.id, file_id: fileId, note });
      } else {
        await createDoc.mutateAsync({
          slot_key: slotKey ?? null,
          title: docTitle.trim() || title,
          category,
          file_id: fileId,
          note: note || null,
        });
      }
      await queryClient.invalidateQueries({ queryKey: ['projects', projectId, 'documents'] });
      toast.success('Document uploaded', 'The antivirus scan runs in the background.');
      setOpen(false);
      setNote('');
    } catch (err) {
      toast.error(
        'Could not attach the file',
        err instanceof Error ? err.message : undefined,
      );
    } finally {
      setAttaching(false);
    }
  };

  return (
    <div className="rounded-md border border-slate-200 bg-white p-4">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 items-start gap-2.5">
          <FileText className="mt-0.5 size-5 shrink-0 text-slate-400" aria-hidden />
          <div className="min-w-0">
            <p className="text-sm font-semibold text-slate-900">
              {document?.title ?? title}
            </p>
            {help && <p className="text-xs text-slate-500">{help}</p>}
            <div className="mt-1 flex flex-wrap items-center gap-2">
              {document && <Badge tone="navy">{titleize(document.category)}</Badge>}
              {version && <StatusBadge status={version.scan_status} label={
                version.scan_status === 'clean'
                  ? 'Scan clean'
                  : version.scan_status === 'rejected'
                    ? 'Scan rejected'
                    : 'Scan pending'
              } />}
            </div>
          </div>
        </div>
        {canUpload && (
          <Button
            type="button"
            size="sm"
            variant="secondary"
            aria-label={`${document ? 'Upload new version' : 'Upload'} — ${title}`}
            icon={<UploadCloud className="size-4" />}
            onClick={() => {
              setDocTitle(document?.title ?? title);
              setOpen(true);
            }}
          >
            {document ? 'Upload new version' : 'Upload'}
          </Button>
        )}
      </div>

      {visibleVersions.length > 0 ? (
        <ul className="mt-3 border-t border-slate-100 pt-2">
          {visibleVersions.map((item) => <li key={item.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-slate-100 py-2 text-sm text-slate-700 last:border-0">
            <span className="font-medium">v{toNum(item.number)}</span>
            <span>by {uploaderName(item.uploaded_by)}</span>
            <span className="text-slate-500">{formatDateTime(item.uploaded_at)}</span>
            <span className="text-slate-500">{formatBytes(item.size)}</span>
            {item.scan_status === 'clean' ? (
              <a
                href={documentVersionDownloadUrl(item.id)}
                className="inline-flex items-center gap-1 font-medium text-teal-700 hover:underline"
              >
                <Download className="size-4" aria-hidden /> Download
              </a>
            ) : (
              <span className="text-xs text-slate-400">
                {item.scan_status === 'rejected'
                  ? 'Download blocked (scan rejected)'
                  : 'Download available after scan'}
              </span>
            )}
            {item.note && <span className="text-xs text-slate-500">Note: {item.note}</span>}
          </li>)}
        </ul>
      ) : (
        <p className="mt-3 border-t border-slate-100 pt-2 text-sm text-slate-500">
          No file uploaded yet.
        </p>
      )}

      <Dialog
        open={open}
        onClose={() => !attaching && setOpen(false)}
        title={document ? `Upload new version — ${document.title}` : `Upload — ${title}`}
      >
        <form
          className="flex flex-col gap-4"
          onSubmit={(e: FormEvent) => e.preventDefault()}
        >
          {!document && (
            <FormField label="Document title" required>
              <Input value={docTitle} onChange={(e) => setDocTitle(e.target.value)} required />
            </FormField>
          )}
          <FormField label="Version note" hint="(optional)">
            <Input
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder="e.g. updated insurance dates"
            />
          </FormField>
          <FileUpload demoMode={demoMode} disabled={attaching} onComplete={onFileReady} />
        </form>
      </Dialog>
    </div>
  );
}
