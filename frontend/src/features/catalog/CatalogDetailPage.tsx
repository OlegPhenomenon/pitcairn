import { Link, useParams } from "react-router";
import { Anchor, Download } from "lucide-react";
import { useApiQuery } from "../../api/client";
import type { PublicProjectDto } from "../../api/types";
import { formatBytes, formatDate, toNum, titleize } from "../../lib/format";
import { Banner, Card, CardBody, EmptyState, Skeleton } from "../../ui";
export function CatalogDetailPage() {
  const { reference } = useParams();
  const project = useApiQuery<PublicProjectDto>(
    ["catalog", reference],
    `/public/projects/${encodeURIComponent(reference ?? "")}`,
    { enabled: !!reference, retry: false },
  );
  return (
    <div className="min-h-dvh bg-sand-50">
      <header className="bg-navy-900 px-4 py-3 text-white">
        <Link
          to="/"
          className="mx-auto flex max-w-6xl items-center gap-2 font-semibold"
        >
          <Anchor className="size-5 text-teal-300" />
          Pitcairn Research Hub
        </Link>
      </header>
      <main id="main" className="mx-auto max-w-4xl px-4 py-8">
        <Link to="/catalog" className="text-sm text-teal-700 underline">
          ← Open catalog
        </Link>
        {project.isPending ? (
          <Skeleton className="mt-6 h-48" />
        ) : project.isError ? (
          <Banner tone="error" className="mt-6">
            {project.error.status === 404
              ? "Project not found."
              : project.error.message}
          </Banner>
        ) : (
          project.data && (
            <>
              <h1 className="mt-5 text-3xl font-bold text-navy-900">
                {project.data.title}
              </h1>
              <p className="text-sm text-slate-600">
                {project.data.reference} · {project.data.organisation} ·{" "}
                {project.data.year
                  ? toNum(project.data.year)
                  : "Year unavailable"}
              </p>
              <p className="mt-4 text-slate-700">{project.data.summary}</p>
              <h2 className="mt-8 mb-3 text-lg font-semibold">
                Published results
              </h2>
              {project.data.deliverables.length === 0 ? (
                <EmptyState title="No published deliverables" />
              ) : (
                <div className="grid gap-3">
                  {project.data.deliverables.map((item) => (
                    <Card key={item.id}>
                      <CardBody>
                        <p className="text-xs text-slate-500">
                          {titleize(item.kind)}
                        </p>
                        <h3 className="font-semibold">{item.title}</h3>
                        <p className="mt-1 text-sm text-slate-600">
                          {item.description}
                        </p>
                        {item.published_at && (
                          <p className="mt-2 text-xs">
                            Published{" "}
                            <time title={item.published_at}>
                              {formatDate(item.published_at)}
                            </time>
                          </p>
                        )}
                        {item.files_available_from && (
                          <p className="mt-2 text-sm text-amber-800">
                            Files available from{" "}
                            <time title={item.files_available_from}>
                              {formatDate(item.files_available_from)}
                            </time>
                          </p>
                        )}
                        {item.files.map((file) => (
                          <a
                            key={file.document_version_id}
                            className="mt-2 flex items-center gap-2 text-sm text-teal-700 underline"
                            href={`/api/v1/public/files/${file.document_version_id}/download`}
                          >
                            <Download className="size-4" />
                            {file.title} ({formatBytes(file.size)})
                          </a>
                        ))}
                      </CardBody>
                    </Card>
                  ))}
                </div>
              )}
            </>
          )
        )}
      </main>
      <footer className="border-t p-4 text-center text-xs text-slate-600">
        Only published material appears here.
      </footer>
    </div>
  );
}
