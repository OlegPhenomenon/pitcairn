import { render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it, vi } from 'vitest';
import type { DocumentDto } from '../../api/generated/DocumentDto';
import type { DocumentVersionDto } from '../../api/generated/DocumentVersionDto';
import { documentVersionDownloadUrl } from '../projects/api';
import { DocumentSlot } from './DocumentSlot';
import type * as UiModule from '../../ui';

vi.mock('../auth/api', () => ({ useMe: () => ({ data: { user: { id: 'me-1' } } }) }));
vi.mock('../../ui', async (orig) => ({
  ...(await orig<typeof UiModule>()),
  useToast: () => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }),
}));

function version(number: number, by: string, name: string): DocumentVersionDto {
  return {
    id: `v${number}`, document_id: 'doc-1', number: BigInt(number), file_id: `f${number}`, note: '',
    uploaded_by: by, uploaded_by_name: name, uploaded_at: `2026-0${number}-01T10:00:00Z`,
    scan_status: 'clean', size: 1024n, mime: 'application/pdf',
  };
}

describe('DocumentSlot', () => {
  it('lists every version with uploader name and download link, newest marked current', () => {
    const versions = [version(2, 'me-1', 'Maria Lopez'), version(1, 'u-2', 'Anna Hart')];
    const document: DocumentDto = {
      id: 'doc-1', project_id: 'p-1', slot_key: 'safety_plan', title: 'Safety plan', category: 'application',
      created_by: 'u-2', created_at: '2026-01-01T10:00:00Z', latest_version: versions[0], versions,
    };
    render(
      <QueryClientProvider client={new QueryClient()}>
        <DocumentSlot projectId="p-1" title="Safety plan" document={document} canUpload={false} />
      </QueryClientProvider>,
    );
    const items = screen.getAllByRole('listitem');
    expect(items).toHaveLength(2);
    expect(within(items[0]).getByText('v2')).toBeTruthy();
    expect(within(items[0]).getByText('Current')).toBeTruthy();
    expect(within(items[0]).getByText('by Maria Lopez (you)')).toBeTruthy();
    expect(within(items[1]).getByText('v1')).toBeTruthy();
    expect(within(items[1]).queryByText('Current')).toBeNull();
    expect(within(items[1]).getByText('by Anna Hart')).toBeTruthy();
    expect(within(items[0]).getByRole('link', { name: /download/i }).getAttribute('href')).toBe(documentVersionDownloadUrl('v2'));
    expect(within(items[1]).getByRole('link', { name: /download/i }).getAttribute('href')).toBe(documentVersionDownloadUrl('v1'));
  });
});
