import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import { MapContainer, TileLayer, useMap } from "react-leaflet";
import '../../lib/leafletIcons';
import { listQuery, useApiQuery } from "../../api/client";
import type { ListResponse, SearchProjectItemDto } from "../../api/types";
import { formatDate, toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  FormField,
  Input,
  PageHeader,
  Select,
  StatusBadge,
  Table,
} from "../../ui";
function AreaButton({ onArea }: { onArea: (bbox: string) => void }) {
  const map = useMap();
  return (
    <button
      type="button"
      className="absolute bottom-3 left-3 z-[400] rounded bg-white px-3 py-2 text-sm font-semibold text-teal-800 shadow"
      onClick={() => {
        const b = map.getBounds();
        onArea(
          [b.getWest(), b.getSouth(), b.getEast(), b.getNorth()].join(","),
        );
      }}
    >
      Search this area
    </button>
  );
}
export function SearchPage() {
  const [q, setQ] = useState(""),
    [org, setOrg] = useState(""),
    [year, setYear] = useState(""),
    [status, setStatus] = useState(""),
    [overdue, setOverdue] = useState(false),
    [bbox, setBbox] = useState(""),
    [filters, setFilters] = useState<Record<string, string>>({});
  const results = useApiQuery<ListResponse<SearchProjectItemDto>>(
    ["search", filters],
    `/search/projects${listQuery(100, 0, filters)}`,
  );
  function search(e: FormEvent) {
    e.preventDefault();
    setFilters({
      q,
      organisation: org,
      year,
      status,
      has_overdue: overdue ? "true" : "",
      bbox,
    });
  }
  return (
    <>
      <PageHeader
        title="Search projects"
        subtitle="Find research by topic, organisation, year or area."
      />
      <form
        onSubmit={search}
        className="grid gap-3 rounded-lg border bg-white p-4 sm:grid-cols-2 lg:grid-cols-5"
      >
        <FormField label="Search">
          <Input value={q} onChange={(e) => setQ(e.target.value)} />
        </FormField>
        <FormField label="Organisation">
          <Input value={org} onChange={(e) => setOrg(e.target.value)} />
        </FormField>
        <FormField label="Year" error={results.error?.fieldError("year")}>
          <Input
            type="number"
            min="1900"
            max="2100"
            value={year}
            onChange={(e) => setYear(e.target.value)}
          />
        </FormField>
        <FormField label="Status" error={results.error?.fieldError("status")}>
          <Select value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="">All statuses</option>
            {[
              "draft",
              "submitted",
              "in_review",
              "changes_requested",
              "approved",
              "refused",
              "closed",
              "withdrawn",
            ].map((s) => (
              <option value={s} key={s}>
                {s.replaceAll("_", " ")}
              </option>
            ))}
          </Select>
        </FormField>
        <div className="flex items-end gap-2">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={overdue}
              onChange={(e) => setOverdue(e.target.checked)}
            />
            Has overdue results
          </label>
          <Button type="submit">Search</Button>
        </div>
      </form>
      <div className="mt-5 overflow-hidden rounded-lg border">
        <MapContainer
          center={[-25.0667, -130.1]}
          zoom={7}
          scrollWheelZoom={false}
          className="h-64 w-full"
        >
          <TileLayer url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png" />
          <AreaButton
            onArea={(b) => {
              setBbox(b);
              setFilters((v) => ({ ...v, bbox: b }));
            }}
          />
        </MapContainer>
      </div>
      {bbox && (
        <p className="mt-2 text-sm">
          Area filter active.{" "}
          <button
            className="text-teal-700 underline"
            onClick={() => {
              setBbox("");
              setFilters((v) => ({ ...v, bbox: "" }));
            }}
          >
            Clear area
          </button>
        </p>
      )}
      <Card className="mt-5">
        <CardBody>
          {results.isError && (
            <Banner tone="error">{results.error.message}</Banner>
          )}
          <Table
            loading={results.isPending}
            rows={results.data?.items ?? []}
            rowKey={(r) => r.id}
            empty={{
              title: "No projects found",
              body: "Try broader search terms or clear the area filter.",
            }}
            columns={[
              {
                header: "Project",
                cell: (r) => (
                  <Link
                    className="font-medium text-teal-700 underline"
                    to={`/app/projects/${r.id}`}
                  >
                    {r.title}
                  </Link>
                ),
              },
              { header: "Reference", cell: (r) => r.reference ?? "—" },
              { header: "Organisation", cell: (r) => r.organisation },
              {
                header: "Status",
                cell: (r) => (
                  <StatusBadge
                    status={r.status}
                    label={
                      r.status === "approved" ? "Permit approved" : undefined
                    }
                  />
                ),
              },
              {
                header: "Dates",
                cell: (r) => (
                  <>
                    <time title={r.start_date ?? ""}>
                      {formatDate(r.start_date)}
                    </time>{" "}
                    –{" "}
                    <time title={r.end_date ?? ""}>
                      {formatDate(r.end_date)}
                    </time>
                  </>
                ),
              },
              {
                header: "Received / overdue",
                cell: (r) =>
                  `${toNum(r.deliverables_received)} / ${toNum(r.deliverables_overdue)}`,
              },
            ]}
          />
        </CardBody>
      </Card>
    </>
  );
}
