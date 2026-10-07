import { createSHA256 } from 'hash-wasm';

import { ApiError, type ApiRequestOptions } from '../../api/client';
import type {
  CompleteUploadResponse,
  UploadStateDto,
} from '../../api/types';
import { toNum } from '../../lib/format';

/**
 * Chunked upload implementing architecture §6:
 * - incremental sha256 of the file (hash-wasm over file.slice pieces)
 * - POST /uploads creates a session (0-based chunks of server chunk_size)
 * - PUT /uploads/{id}/chunks/{n} raw bytes, retries with backoff
 * - resume: upload id kept in storage keyed by name+size+lastModified;
 *   GET /uploads/{id} lists chunks_received which are skipped
 * - POST /uploads/{id}/complete → {file_id, scan_status}
 * - DELETE /uploads/{id} cancels
 */

export type UploadPhase =
  | 'idle'
  | 'hashing'
  | 'creating'
  | 'uploading'
  | 'paused'
  | 'completing'
  | 'done'
  | 'error'
  | 'cancelled';

export interface UploadProgress {
  phase: UploadPhase;
  /** Bytes confirmed by the server (whole chunks). */
  bytesConfirmed: number;
  totalBytes: number;
  percent: number;
  /** 0–100 while hashing. */
  hashPercent: number;
  chunkIndex: number | null;
  chunksDone: number;
  chunksTotal: number;
  attempt: number;
  uploadId: string | null;
  fileId: string | null;
  scanStatus: string | null;
  error: ApiError | null;
}

export const initialProgress: UploadProgress = {
  phase: 'idle',
  bytesConfirmed: 0,
  totalBytes: 0,
  percent: 0,
  hashPercent: 0,
  chunkIndex: null,
  chunksDone: 0,
  chunksTotal: 0,
  attempt: 0,
  uploadId: null,
  fileId: null,
  scanStatus: null,
  error: null,
};

export interface FileLike {
  name: string;
  size: number;
  lastModified?: number;
  type?: string;
  slice(start: number, end: number): Blob;
}

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface UploaderDeps {
  /** JSON API request — in prod, `api` from api/client; in tests, a mock. */
  request: <T>(path: string, opts?: ApiRequestOptions) => Promise<T>;
  storage: StorageLike;
  sleep?: (ms: number) => Promise<void>;
  /** Retries per chunk on network/5xx errors (default 4). */
  maxRetries?: number;
  /** Bytes hashed per slice while computing sha256 (default 4 MiB). */
  hashStep?: number;
}

const defaultSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function uploadStorageKey(file: FileLike): string {
  return `pitcairn-upload:${file.name}:${file.size}:${file.lastModified ?? 0}`;
}

/** Incremental sha256 of a File/Blob using hash-wasm over slices. */
export async function hashFile(
  file: FileLike,
  step = 4 * 1024 * 1024,
  onProgress?: (frac: number) => void,
): Promise<string> {
  const hasher = await createSHA256();
  hasher.init();
  let offset = 0;
  while (offset < file.size) {
    const blob = file.slice(offset, Math.min(offset + step, file.size));
    const buf = new Uint8Array(await blob.arrayBuffer());
    hasher.update(buf);
    offset += buf.byteLength;
    onProgress?.(Math.min(1, offset / Math.max(1, file.size)));
  }
  return hasher.digest('hex');
}

function chunkSizeFor(state: UploadStateDto, n: number, fileSize: number): number {
  const total = toNum(state.chunks_total);
  const size = toNum(state.chunk_size);
  return n === total - 1 ? fileSize - (total - 1) * size : size;
}

function receivedBytes(state: UploadStateDto, fileSize: number): number {
  return state.chunks_received.reduce(
    (sum, n) => sum + chunkSizeFor(state, toNum(n), fileSize),
    0,
  );
}

function isRetryable(err: unknown): boolean {
  if (!(err instanceof ApiError)) return true; // network failure
  if (err.status === 0) return true;
  if (err.status === 429) return true;
  if (err.status >= 500) return true;
  return false; // 4xx (bad_chunk_length, checksum, authz…) — never retried
}

export class ChunkedUploader {
  private progress: UploadProgress = { ...initialProgress, totalBytes: 0 };
  private listeners = new Set<(p: UploadProgress) => void>();
  private paused = false;
  private cancelled = false;
  private abort: AbortController | null = null;
  private sleep: (ms: number) => Promise<void>;

  constructor(
    private file: FileLike,
    private deps: UploaderDeps,
  ) {
    this.progress.totalBytes = file.size;
    this.sleep = deps.sleep ?? defaultSleep;
  }

  onProgress(cb: (p: UploadProgress) => void): () => void {
    this.listeners.add(cb);
    cb(this.progress);
    return () => this.listeners.delete(cb);
  }

  get state(): UploadProgress {
    return this.progress;
  }

  private emit(patch: Partial<UploadProgress>) {
    this.progress = { ...this.progress, ...patch };
    for (const cb of this.listeners) cb(this.progress);
  }

  private key() {
    return uploadStorageKey(this.file);
  }

  pause() {
    if (this.progress.phase !== 'uploading') return;
    this.paused = true;
    this.abort?.abort();
    this.emit({ phase: 'paused' });
  }

  resume() {
    if (this.progress.phase !== 'paused') return;
    this.paused = false;
    void this.run(); // continue from the stored session
  }

  async cancel() {
    this.cancelled = true;
    this.abort?.abort();
    const id = this.progress.uploadId;
    this.deps.storage.removeItem(this.key());
    if (id) {
      try {
        await this.deps.request(`/uploads/${id}`, { method: 'DELETE' });
      } catch {
        // already gone / aborted — fine
      }
    }
    this.emit({ phase: 'cancelled' });
  }

  /** Begin (or resume) the upload; resolves with the file id on success. */
  async start(): Promise<string> {
    this.cancelled = false;
    this.paused = false;
    return this.run();
  }

  private async run(): Promise<string> {
    try {
      const fileId = await this.doRun();
      this.emit({ phase: 'done', fileId, percent: 100 });
      return fileId;
    } catch (err) {
      if (this.cancelled) {
        this.emit({ phase: 'cancelled' });
        throw err;
      }
      if (this.paused) {
        // paused mid-flight — emit() already happened
        throw err;
      }
      const apiErr =
        err instanceof ApiError
          ? err
          : new ApiError(0, 'upload_failed', 'Upload failed unexpectedly.');
      this.emit({ phase: 'error', error: apiErr });
      throw apiErr;
    }
  }

  private async doRun(): Promise<string> {
    // 1. resume an existing session if we have one
    let state = await this.resumeSession();

    // 2. hash + create session
    if (!state) {
      this.emit({ phase: 'hashing', hashPercent: 0 });
      const sha256 = await hashFile(this.file, this.deps.hashStep, (f) =>
        this.emit({ hashPercent: Math.round(f * 100) }),
      );
      if (this.cancelled) throw new ApiError(0, 'cancelled', 'cancelled');
      this.emit({ phase: 'creating' });
      state = await this.deps.request<UploadStateDto>('/uploads', {
        method: 'POST',
        body: {
          filename: this.file.name,
          size: BigInt(this.file.size),
          sha256,
          mime: this.file.type || 'application/octet-stream',
        },
      });
      this.deps.storage.setItem(this.key(), state.upload_id);
      if (this.cancelled) {
        this.emit({ uploadId: state.upload_id });
        await this.cancel();
        throw new ApiError(0, 'cancelled', 'Upload cancelled');
      }
    }
    this.emit({
      uploadId: state.upload_id,
      chunksTotal: toNum(state.chunks_total),
      chunksDone: state.chunks_received.length,
      bytesConfirmed: receivedBytes(state, this.file.size),
    });
    this.updatePercent();

    // Upload already completed server-side (e.g. page reload after complete).
    if (state.status === 'complete' && state.file_id) {
      this.deps.storage.removeItem(this.key());
      this.emit({ scanStatus: null, fileId: state.file_id });
      return state.file_id;
    }
    if (state.status === 'aborted') {
      this.deps.storage.removeItem(this.key());
      // fall through to a fresh session on the next start()
      throw new ApiError(409, 'upload_aborted', 'Upload was aborted — start again.');
    }

    // 3. send missing chunks
    const received = new Set(state.chunks_received.map((n) => toNum(n)));
    const total = toNum(state.chunks_total);
    const chunkSize = toNum(state.chunk_size);
    this.emit({ phase: 'uploading' });

    for (let n = 0; n < total; n++) {
      if (received.has(n)) continue;
      while (this.paused) {
        await this.sleep(150);
        if (this.cancelled) throw new ApiError(0, 'cancelled', 'cancelled');
      }
      if (this.cancelled) throw new ApiError(0, 'cancelled', 'cancelled');

      const start = n * chunkSize;
      const end = Math.min(start + chunkSize, this.file.size);
      const bytes = new Uint8Array(
        await this.file.slice(start, end).arrayBuffer(),
      );
      await this.putChunk(state.upload_id, n, bytes);
      received.add(n);
      this.emit({
        chunksDone: received.size,
        bytesConfirmed:
          this.progress.bytesConfirmed + (end - start),
        chunkIndex: n,
      });
      this.updatePercent();
    }

    // 4. complete → server verifies size + sha256
    this.emit({ phase: 'completing' });
    const done = await this.deps.request<CompleteUploadResponse>(
      `/uploads/${state.upload_id}/complete`,
      { method: 'POST' },
    );
    this.deps.storage.removeItem(this.key());
    this.emit({ fileId: done.file_id, scanStatus: done.scan_status });
    return done.file_id;
  }

  private updatePercent() {
    const total = Math.max(1, this.progress.totalBytes);
    this.emit({
      percent: Math.min(100, Math.round((this.progress.bytesConfirmed / total) * 100)),
    });
  }

  private async putChunk(uploadId: string, n: number, bytes: Uint8Array) {
    const maxRetries = this.deps.maxRetries ?? 4;
    for (let attempt = 0; ; attempt++) {
      if (this.cancelled) throw new ApiError(0, 'cancelled', 'cancelled');
      if (this.paused) throw new ApiError(0, 'paused', 'paused');
      this.abort = new AbortController();
      try {
        await this.deps.request(`/uploads/${uploadId}/chunks/${n}`, {
          method: 'PUT',
          rawBody: bytes as unknown as BodyInit,
          headers: { 'Content-Type': 'application/octet-stream' },
          signal: this.abort.signal,
        });
        this.emit({ attempt: 0 });
        return;
      } catch (err) {
        this.abort = null;
        if (this.cancelled) throw new ApiError(0, 'cancelled', 'cancelled');
        if (this.paused) throw new ApiError(0, 'paused', 'paused');
        if (!isRetryable(err) || attempt >= maxRetries) {
          // ApiError passes through untouched so callers see the real code.
          throw err instanceof ApiError
            ? err
            : new ApiError(0, 'network_error', 'Network error during upload.');
        }
        this.emit({ attempt: attempt + 1 });
        await this.sleep(Math.min(8000, 500 * 2 ** attempt));
      }
    }
  }

  /** GET /uploads/{id} for a stored session; returns null if unusable. */
  private async resumeSession(): Promise<UploadStateDto | null> {
    const id = this.deps.storage.getItem(this.key());
    if (!id) return null;
    try {
      const state = await this.deps.request<UploadStateDto>(`/uploads/${id}`);
      if (state.status === 'open' || (state.status === 'complete' && state.file_id)) {
        return state;
      }
      this.deps.storage.removeItem(this.key());
      return null;
    } catch (err) {
      if (err instanceof ApiError && (err.status === 404 || err.status === 403)) {
        this.deps.storage.removeItem(this.key());
        return null;
      }
      throw err;
    }
  }
}
