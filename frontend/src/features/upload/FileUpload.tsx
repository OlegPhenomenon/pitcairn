import { useRef, useState, type DragEvent } from 'react';
import { FileUp, Pause, Play, XCircle } from 'lucide-react';

import { ApiError } from '../../api/client';
import { formatBytes } from '../../lib/format';
import { cx } from '../../lib/cx';
import { Banner, Button, Checkbox } from '../../ui';
import { useChunkedUpload } from './useChunkedUpload';

function friendlyError(err: ApiError | null): string {
  if (!err) return '';
  switch (err.code) {
    case 'checksum_mismatch':
      return 'The uploaded file did not match its checksum — the file may have changed during upload. Please try again.';
    case 'bad_chunk_length':
      return 'A chunk had the wrong size — please try again.';
    case 'bad_chunk_index':
      return 'The upload got out of order — please try again.';
    case 'missing_chunks':
      return err.message;
    case 'upload_aborted':
      return 'This upload was aborted. Start over with the same file.';
    case 'network_error':
      return 'Network error during upload. Check your connection and resume.';
    default:
      return err.message || 'Upload failed.';
  }
}

export interface FileUploadProps {
  /** Called with the file_id once the upload completes. */
  onComplete: (fileId: string) => void;
  /** Demo mode: require "This is fictional test data" before uploading. */
  demoMode?: boolean;
  disabled?: boolean;
  /** Extra label text for the drop zone. */
  hint?: string;
}

/**
 * Drag & drop + chunked upload widget (§6): hashing, resumable session,
 * pause/resume/cancel, progress, clear error messages.
 */
export function FileUpload({ onComplete, demoMode, disabled, hint }: FileUploadProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [file, setFile] = useState<File | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [demoOk, setDemoOk] = useState(false);
  const up = useChunkedUpload();
  const p = up.progress;

  const busy = ['hashing', 'creating', 'uploading', 'paused', 'completing'].includes(
    p.phase,
  );

  const choose = (f: File | null | undefined) => {
    if (!f) return;
    setFile(f);
    up.reset();
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    if (disabled || busy) return;
    choose(e.dataTransfer.files?.[0]);
  };

  const start = async () => {
    if (!file) return;
    try {
      const fileId = await up.upload(file);
      onComplete(fileId);
    } catch {
      // error state is in progress.error; cancelled just resets
    }
  };

  const errorText =
    p.phase === 'error' && p.error && p.error.code !== 'cancelled'
      ? friendlyError(p.error)
      : null;

  return (
    <div className="flex flex-col gap-3">
      <div
        onDragOver={(e) => {
          e.preventDefault();
          if (!disabled && !busy) setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
        className={cx(
          'flex flex-col items-center gap-2 rounded-lg border-2 border-dashed px-4 py-6 text-center',
          dragOver ? 'border-teal-600 bg-teal-50' : 'border-slate-300 bg-slate-50/60',
          (disabled || busy) && 'opacity-60',
        )}
      >
        <FileUp className="size-7 text-slate-400" aria-hidden />
        {file ? (
          <p className="text-sm font-medium break-all text-slate-800">
            {file.name} <span className="font-normal text-slate-500">({formatBytes(file.size)})</span>
          </p>
        ) : (
          <p className="text-sm text-slate-600">
            Drag a file here, or{' '}
            <button
              type="button"
              className="font-medium text-teal-700 underline"
              onClick={() => inputRef.current?.click()}
              disabled={disabled || busy}
            >
              choose a file
            </button>
          </p>
        )}
        {file && !busy && (
          <button
            type="button"
            className="text-xs text-slate-500 underline"
            onClick={() => inputRef.current?.click()}
            disabled={disabled}
          >
            choose a different file
          </button>
        )}
        {hint && <p className="text-xs text-slate-500">{hint}</p>}
        <input
          ref={inputRef}
          type="file"
          className="sr-only"
          aria-label="Choose file to upload"
          onChange={(e) => choose(e.target.files?.[0])}
          disabled={disabled || busy}
        />
      </div>

      {demoMode && (
        <Checkbox
          label="This is fictional test data"
          checked={demoOk}
          onChange={(e) => setDemoOk(e.target.checked)}
          disabled={disabled || busy}
          required
        />
      )}

      {busy && (
        <div className="flex flex-col gap-1.5">
          <div
            role="progressbar"
            aria-valuenow={p.phase === 'hashing' ? p.hashPercent : p.percent}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label="Upload progress"
            className="h-2 overflow-hidden rounded-full bg-slate-200"
          >
            <div
              className="h-full rounded-full bg-teal-600 motion-safe:transition-[width]"
              style={{
                width: `${p.phase === 'hashing' ? p.hashPercent : p.percent}%`,
              }}
            />
          </div>
          <p className="text-xs text-slate-600">
            {p.phase === 'hashing' && `Reading file… ${p.hashPercent}%`}
            {p.phase === 'creating' && 'Creating upload…'}
            {p.phase === 'uploading' &&
              `Uploading — ${p.percent}% (${formatBytes(p.bytesConfirmed)} of ${formatBytes(p.totalBytes)})${
                p.attempt > 0 ? ` · retry ${p.attempt}` : ''
              }`}
            {p.phase === 'paused' && 'Paused — resume to continue'}
            {p.phase === 'completing' && 'Finishing — verifying checksum…'}
          </p>
        </div>
      )}

      {errorText && <Banner tone="error">{errorText}</Banner>}
      {p.phase === 'done' && (
        <Banner tone="info">Uploaded — antivirus scan pending.</Banner>
      )}
      {p.phase === 'cancelled' && <Banner tone="warning">Upload cancelled.</Banner>}

      <div className="flex flex-wrap gap-2">
        {!busy && (
          <Button
            type="button"
            onClick={start}
            disabled={!file || disabled || (demoMode && !demoOk)}
          >
            Upload
          </Button>
        )}
        {p.phase === 'uploading' && (
          <Button type="button" variant="secondary" size="sm" icon={<Pause className="size-4" />} onClick={up.pause}>
            Pause
          </Button>
        )}
        {p.phase === 'paused' && (
          <Button type="button" variant="secondary" size="sm" icon={<Play className="size-4" />} onClick={up.resume}>
            Resume
          </Button>
        )}
        {busy && (
          <Button type="button" variant="ghost" size="sm" icon={<XCircle className="size-4" />} onClick={up.cancel}>
            Cancel upload
          </Button>
        )}
        {p.phase === 'error' && (
          <Button type="button" variant="secondary" size="sm" onClick={up.reset}>
            Dismiss
          </Button>
        )}
      </div>
    </div>
  );
}
