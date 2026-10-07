import { describe, expect, it, vi } from 'vitest';
import { ApiError, type ApiRequestOptions } from '../../api/client';
import { ChunkedUploader, uploadStorageKey, type FileLike } from './uploader';

function makeFile(): FileLike {
  const data = new TextEncoder().encode('abcdef');
  return {
    name: 'sample.txt', size: data.length, type: 'text/plain', lastModified: 123,
    slice: (start, end) => ({ arrayBuffer: async () => data.slice(start, end).buffer }) as Blob,
  };
}

function memoryStorage() {
  const entries = new Map<string, string>();
  return {
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => { entries.set(key, value); },
    removeItem: (key: string) => { entries.delete(key); },
  };
}

describe('ChunkedUploader', () => {
  it('resumes a stored session and skips chunks already received', async () => {
    const file = makeFile();
    const storage = memoryStorage();
    storage.setItem(uploadStorageKey(file), 'session-1');
    const request = vi.fn(async (path: string, opts?: ApiRequestOptions): Promise<never> => {
      if (path === '/uploads/session-1') return { upload_id: 'session-1', chunk_size: 2n, chunks_total: 3n, chunks_received: [0n, 2n], status: 'open', file_id: null } as never;
      if (path === '/uploads/session-1/chunks/1') {
        expect(opts?.method).toBe('PUT');
        expect(new TextDecoder().decode(opts?.rawBody as Uint8Array)).toBe('cd');
        return undefined as never;
      }
      if (path === '/uploads/session-1/complete') return { file_id: 'file-1', scan_status: 'pending' } as never;
      throw new Error(`Unexpected ${path}`);
    });
    const uploader = new ChunkedUploader(file, { request, storage });
    expect(await uploader.start()).toBe('file-1');
    expect(request.mock.calls.map(([path]) => path)).toEqual(['/uploads/session-1', '/uploads/session-1/chunks/1', '/uploads/session-1/complete']);
    expect(storage.getItem(uploadStorageKey(file))).toBeNull();
  });

  it('surfaces a checksum mismatch without clearing the resumable session', async () => {
    const file = makeFile();
    const storage = memoryStorage();
    storage.setItem(uploadStorageKey(file), 'session-2');
    const request = vi.fn(async (path: string): Promise<never> => {
      if (path === '/uploads/session-2') return { upload_id: 'session-2', chunk_size: 6n, chunks_total: 1n, chunks_received: [0n], status: 'open', file_id: null } as never;
      if (path === '/uploads/session-2/complete') throw new ApiError(422, 'checksum_mismatch', 'Checksum mismatch');
      throw new Error(`Unexpected ${path}`);
    });
    const uploader = new ChunkedUploader(file, { request, storage });
    await expect(uploader.start()).rejects.toMatchObject({ code: 'checksum_mismatch' });
    expect(uploader.state.error?.code).toBe('checksum_mismatch');
    expect(storage.getItem(uploadStorageKey(file))).toBe('session-2');
  });
});
