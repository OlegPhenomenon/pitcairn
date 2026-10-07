import { useCallback, useRef, useState } from 'react';

import { api, ApiError } from '../../api/client';
import {
  ChunkedUploader,
  initialProgress,
  type UploadProgress,
} from './uploader';

export interface ChunkedUpload {
  progress: UploadProgress;
  /** Start (or resume) uploading `file`. Resolves with the file id. */
  upload: (file: File) => Promise<string>;
  pause: () => void;
  resume: () => void;
  cancel: () => void;
  /** Reset progress to idle (after an error/done acknowledgement). */
  reset: () => void;
  active: boolean;
}

/**
 * React wrapper around ChunkedUploader: keeps progress in state, wires the
 * real `api` client + localStorage for resume keys.
 */
export function useChunkedUpload(): ChunkedUpload {
  const [progress, setProgress] = useState<UploadProgress>(initialProgress);
  const uploaderRef = useRef<ChunkedUploader | null>(null);

  const upload = useCallback((file: File) => {
    uploaderRef.current?.cancel().catch(() => undefined);
    const uploader = new ChunkedUploader(file, {
      request: api,
      storage: window.localStorage,
    });
    uploaderRef.current = uploader;

    return new Promise<string>((resolve, reject) => {
      uploader.onProgress((p) => {
        setProgress(p);
        if (p.phase === 'done' && p.fileId) resolve(p.fileId);
        else if (p.phase === 'error') reject(p.error ?? new ApiError(0, 'upload_failed', 'Upload failed'));
        else if (p.phase === 'cancelled')
          reject(new ApiError(0, 'cancelled', 'Upload cancelled'));
      });
      uploader.start().catch(() => {
        // error/cancelled/paused states are surfaced through onProgress
      });
    });
  }, []);

  const pause = useCallback(() => uploaderRef.current?.pause(), []);
  const resume = useCallback(() => uploaderRef.current?.resume(), []);
  const cancel = useCallback(() => {
    void uploaderRef.current?.cancel();
  }, []);
  const reset = useCallback(() => setProgress(initialProgress), []);

  const active = ['hashing', 'creating', 'uploading', 'paused', 'completing'].includes(
    progress.phase,
  );

  return { progress, upload, pause, resume, cancel, reset, active };
}
