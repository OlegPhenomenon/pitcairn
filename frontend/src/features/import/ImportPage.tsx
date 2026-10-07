import { useState } from "react";
import { api, apiPost, useApiMutation } from "../../api/client";
import type {
  ArchiveImportPreviewResponse,
  ImportCommitResponse,
  LegacyImportPreviewResponse,
} from "../../api/types";
import { formatBytes, toNum } from "../../lib/format";
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

const LEGACY_COLUMNS = [
  "reference",
  "title",
  "organisation",
  "lead_name",
  "lead_email",
  "start_date",
  "end_date",
  "summary",
  "keywords",
  "site_name",
  "lat",
  "lng",
  "report_title",
  "report_url",
  "report_file",
  "dataset_title",
  "dataset_file",
  "application_file",
];

const LEGACY_EXAMPLE = [
  "MSB-2009-001",
  "Lobster census at Bounty Bay",
  "Bounty Lobster Trust (fictional)",
  "Dr Ada Christian",
  "a.christian@example.org",
  "2009-03-02",
  "2009-03-30",
  "Night dive census of spiny lobsters",
  "lobsters; census",
  "Bounty Bay",
  "-25.066",
  "-130.104",
  "Lobster census report 2009",
  "",
  "report.pdf",
  "Census counts",
  "data/counts.csv;data/sites.csv",
  "application-v1.pdf;application-v2.pdf",
];

const FILE_COLUMN_LABELS: Record<string, string> = {
  application_file: "Application",
  report_file: "Report",
  dataset_file: "Dataset",
};

function downloadLegacyTemplate() {
  // The template values contain no commas or quotes, so no CSV quoting.
  const text = [LEGACY_COLUMNS, LEGACY_EXAMPLE]
    .map((row) => row.join(","))
    .join("\n");
  const url = URL.createObjectURL(
    new Blob([`${text}\n`], { type: "text/csv" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = "legacy-import-template.csv";
  link.click();
  URL.revokeObjectURL(url);
}

export function ImportPage() {
  const toast = useToast(),
    me = useMe();
  const [legacyFile, setLegacyFile] = useState<File | null>(null),
    [zip, setZip] = useState<File | null>(null),
    [legacy, setLegacy] = useState<LegacyImportPreviewResponse | null>(null),
    [archive, setArchive] = useState<ArchiveImportPreviewResponse | null>(null),
    [confirm, setConfirm] = useState<"legacy" | "archive" | null>(null),
    [fictional, setFictional] = useState(false);
  const previewLegacy = useApiMutation<LegacyImportPreviewResponse, File>(
    (file) =>
      api("/admin/import/legacy/preview", {
        method: "POST",
        rawBody: file,
        headers: {
          "Content-Type": /\.zip$/i.test(file.name)
            ? "application/zip"
            : "text/csv",
        },
      }),
    {
      onSuccess: (data) => {
        setLegacy(data);
        toast.success("Legacy import preview ready");
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
          <CardHeader
            title="Legacy records (CSV or ZIP)"
            actions={
              <Button
                size="sm"
                variant="secondary"
                onClick={downloadLegacyTemplate}
              >
                Download CSV template
              </Button>
            }
          />
          <CardBody className="space-y-3">
            <p className="text-sm text-slate-600">
              Columns: reference, title, organisation, lead name and email,
              dates, summary, keywords, site, report and dataset. To bring old
              applications, reports and data files along, upload a ZIP with the
              CSV at the top and the files in a <code>files/</code> folder; name
              them in <code>application_file</code> (versions, oldest first,
              separated by <code>;</code>), <code>report_file</code> and{" "}
              <code>dataset_file</code>.
            </p>
            <input
              type="file"
              accept=".csv,.zip,text/csv,application/zip"
              aria-label="Choose legacy CSV or ZIP"
              onChange={(e) => setLegacyFile(e.target.files?.[0] ?? null)}
            />
            <Button
              disabled={
                !legacyFile ||
                (Boolean(me.data?.demo_mode) && !fictional) ||
                previewLegacy.isPending
              }
              onClick={() =>
                legacyFile && void previewLegacy.mutateAsync(legacyFile)
              }
            >
              {previewLegacy.isPending
                ? "Reading and checking records…"
                : "Preview legacy import"}
            </Button>
            {previewLegacy.error && (
              <Banner tone="error">{previewLegacy.error.message}</Banner>
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
            title={`Legacy import preview · ${legacy.rows.length} rows`}
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
                CSV or files and preview again before committing.
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
                  header: "Files",
                  cell: (r) =>
                    r.files.length === 0 ? (
                      "—"
                    ) : (
                      <ul className="space-y-0.5 text-sm">
                        {r.files.map((f) => (
                          <li key={`${f.column}:${f.name}`}>
                            {FILE_COLUMN_LABELS[f.column] ?? f.column}: {f.name}{" "}
                            <span className="text-slate-500">
                              ({formatBytes(f.size)})
                            </span>
                          </li>
                        ))}
                      </ul>
                    ),
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
