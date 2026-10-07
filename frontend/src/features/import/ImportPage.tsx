import { useState } from "react";
import { api, apiPost, useApiMutation } from "../../api/client";
import type {
  ArchiveImportPreviewResponse,
  ImportCommitResponse,
  LegacyImportPreviewResponse,
} from "../../api/types";
import { toNum } from "../../lib/format";
import { useMe } from "../auth/api";
import {
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  Checkbox,
  Dialog,
  EmptyState,
  PageHeader,
  StatusBadge,
  Table,
  useToast,
} from "../../ui";
export function ImportPage() {
  const toast = useToast(),
    me = useMe();
  const [csv, setCsv] = useState<File | null>(null),
    [zip, setZip] = useState<File | null>(null),
    [legacy, setLegacy] = useState<LegacyImportPreviewResponse | null>(null),
    [archive, setArchive] = useState<ArchiveImportPreviewResponse | null>(null),
    [confirm, setConfirm] = useState<"legacy" | "archive" | null>(null),
    [fictional, setFictional] = useState(false);
  const previewCsv = useApiMutation<LegacyImportPreviewResponse, File>(
    (file) =>
      api("/admin/import/legacy/preview", {
        method: "POST",
        rawBody: file,
        headers: { "Content-Type": "text/csv" },
      }),
    {
      onSuccess: (data) => {
        setLegacy(data);
        toast.success("CSV preview ready");
      },
    },
  );
  const previewZip = useApiMutation<ArchiveImportPreviewResponse, File>(
    (file) =>
      api("/admin/import/project-archive", {
        method: "POST",
        rawBody: file,
        headers: { "Content-Type": "application/zip" },
      }),
    {
      onSuccess: (data) => {
        setArchive(data);
        toast.success("Archive preview ready");
      },
    },
  );
  const commit = useApiMutation<ImportCommitResponse, string>(
    (id) => apiPost(`/admin/import/${id}/commit`),
    {
      onSuccess: (data) => {
        toast.success(
          "Import committed",
          `${toNum(data.created)} created, ${toNum(data.skipped)} skipped`,
        );
        setConfirm(null);
        setLegacy(null);
        setArchive(null);
      },
    },
  );
  const badRows =
    legacy?.rows.filter((r) => Object.keys(r.errors).length || r.duplicate_of)
      .length ?? 0;
  return (
    <>
      <PageHeader
        title="Import projects"
        subtitle="Preview imported records and conflicts before committing."
      />
      {me.data?.demo_mode && (
        <label className="mb-4 flex items-center gap-2 text-sm">
          <Checkbox
            checked={fictional}
            onChange={(e) => setFictional(e.target.checked)}
          />
          This is fictional test data
        </label>
      )}
      <div className="grid gap-5 lg:grid-cols-2">
        <Card>
          <CardHeader title="Legacy CSV" />
          <CardBody className="space-y-3">
            <p className="text-sm text-slate-600">
              Columns: reference, title, organisation, lead name and email,
              dates, summary, keywords, site and report.
            </p>
            <input
              type="file"
              accept=".csv,text/csv"
              aria-label="Choose legacy CSV"
              onChange={(e) => setCsv(e.target.files?.[0] ?? null)}
            />
            <Button
              disabled={
                !csv ||
                (Boolean(me.data?.demo_mode) && !fictional) ||
                previewCsv.isPending
              }
              onClick={() => csv && void previewCsv.mutateAsync(csv)}
            >
              {previewCsv.isPending
                ? "Reading and checking CSV…"
                : "Preview CSV"}
            </Button>
            {previewCsv.error && (
              <Banner tone="error">{previewCsv.error.message}</Banner>
            )}
          </CardBody>
        </Card>
        <Card>
          <CardHeader title="Project archive ZIP" />
          <CardBody className="space-y-3">
            <p className="text-sm text-slate-600">
              Import a Pitcairn project archive exported from another
              installation.
            </p>
            <input
              type="file"
              accept=".zip,application/zip"
              aria-label="Choose project archive ZIP"
              onChange={(e) => setZip(e.target.files?.[0] ?? null)}
            />
            <Button
              disabled={
                !zip ||
                (Boolean(me.data?.demo_mode) && !fictional) ||
                previewZip.isPending
              }
              onClick={() => zip && void previewZip.mutateAsync(zip)}
            >
              {previewZip.isPending ? "Checking archive…" : "Preview archive"}
            </Button>
            {previewZip.error && (
              <Banner tone="error">{previewZip.error.message}</Banner>
            )}
          </CardBody>
        </Card>
      </div>
      {legacy && (
        <Card className="mt-5">
          <CardHeader
            title={`CSV preview · ${legacy.rows.length} rows`}
            actions={
              <Button
                disabled={badRows > 0}
                onClick={() => setConfirm("legacy")}
              >
                Commit import
              </Button>
            }
          />
          <CardBody>
            {badRows > 0 && (
              <Banner tone="warning">
                {badRows} row(s) have errors or possible duplicates. Correct the
                CSV and preview it again before committing.
              </Banner>
            )}
            <Table
              rows={legacy.rows}
              rowKey={(r) => toNum(r.index)}
              empty={{ title: "CSV has no rows" }}
              columns={[
                { header: "Row", cell: (r) => toNum(r.index) },
                { header: "Reference", cell: (r) => r.data.reference ?? "—" },
                { header: "Title", cell: (r) => r.data.title ?? "—" },
                {
                  header: "Organisation",
                  cell: (r) => r.data.organisation ?? "—",
                },
                {
                  header: "Issues",
                  cell: (r) => (
                    <div className="text-red-700">
                      {Object.entries(r.errors).map(([k, v]) => (
                        <p key={k}>
                          {k}: {v}
                        </p>
                      ))}
                      {r.duplicate_of && (
                        <p>Possible duplicate of {r.duplicate_of}</p>
                      )}
                    </div>
                  ),
                },
              ]}
            />
          </CardBody>
        </Card>
      )}
      {archive && (
        <Card className="mt-5">
          <CardHeader
            title="Archive preview"
            actions={
              <Button
                disabled={archive.conflicts.length > 0}
                onClick={() => setConfirm("archive")}
              >
                Commit archive
              </Button>
            }
          />
          <CardBody>
            <p className="font-medium">
              {archive.project_reference} · {archive.project_title}
            </p>
            <p className="text-sm">{toNum(archive.files)} files</p>
            <dl className="mt-2 grid gap-2 sm:grid-cols-3">
              {Object.entries(archive.records).map(([name, count]) => (
                <div key={name}>
                  <dt className="text-xs text-slate-500">{name}</dt>
                  <dd>{count}</dd>
                </div>
              ))}
            </dl>
            {archive.conflicts.length > 0 ? (
              <Banner tone="warning">{archive.conflicts.join("; ")}</Banner>
            ) : (
              <StatusBadge status="clean" label="No conflicts" />
            )}
            <p className="mt-2 text-sm">
              {archive.matched_users.length} existing users matched;{" "}
              {archive.new_users.length} disabled user stubs will be created.
            </p>
          </CardBody>
        </Card>
      )}
      {!legacy && !archive && (
        <EmptyState
          className="mt-5"
          title="No preview yet"
          body="Choose a CSV or project ZIP to see what will be imported."
        />
      )}
      <Dialog
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title="Commit import"
        footer={
          <>
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              disabled={commit.isPending}
              onClick={() => {
                const id =
                  confirm === "legacy" ? legacy?.batch.id : archive?.batch.id;
                if (id) void commit.mutateAsync(id);
              }}
            >
              {commit.isPending ? "Importing…" : "Commit import"}
            </Button>
          </>
        }
      >
        <p>
          These records will be added to this installation. Review the preview
          before continuing.
        </p>
        {commit.error && <Banner tone="error">{commit.error.message}</Banner>}
      </Dialog>
    </>
  );
}
