import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router";
import { MapContainer, TileLayer, Rectangle, useMap } from "react-leaflet";
import { formatBbox, parseBbox, type Bbox } from "./bbox";
import '../../lib/leafletIcons';
import { listQuery, useApiQuery } from "../../api/client";
import type { ListResponse, SearchProjectItemDto } from "../../api/types";
import { formatDate, toNum } from "../../lib/format";
import {
  Badge,
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
function FitToBbox({ bbox }: { bbox: Bbox | null }) {
  const map = useMap();
  useEffect(() => {
    if (bbox) map.fitBounds([[bbox[1], bbox[0]], [bbox[3], bbox[2]]], { maxZoom: 14 });
  }, [map, bbox]);
  return null;
}
function AreaButton({ onArea }: { onArea: (bbox: Bbox) => void }) {
  const map = useMap();
  return (
    <button
      type="button"
      className="absolute bottom-3 left-3 z-[400] rounded bg-white px-3 py-2 text-sm font-semibold text-teal-800 shadow"
      onClick={() => {
        const b = map.getBounds();
        onArea([b.getWest(), b.getSouth(), b.getEast(), b.getNorth()]);
      }}
    >
      Search this area
    </button>
  );
}
export function SearchPage() {
  const [params, setParams] = useSearchParams();
  const bboxParam = params.get("bbox");
  const bbox = parseBbox(bboxParam);
  const bboxKey = bbox ? formatBbox(bbox) : "";
  const currentProject = params.get("project");
  const [q, setQ] = useState(""),
    [org, setOrg] = useState(""),
    [year, setYear] = useState(""),
    [status, setStatus] = useState(""),
    [overdue, setOverdue] = useState(false),
    [filters, setFilters] = useState<Record<string, string>>({});
  const query = { ...filters, bbox: bboxKey };
  const results = useApiQuery<ListResponse<SearchProjectItemDto>>(
    ["search", query],
    `/search/projects${listQuery(100, 0, query)}`,
  );
  // Re-parse from the stable string so the map only refits when the area changes.
  const fitBbox = useMemo(() => parseBbox(bboxKey), [bboxKey]);
  function setArea(next: Bbox | null) {
    setParams(
      (p) => {
        const out = new URLSearchParams(p);
        if (next) out.set("bbox", formatBbox(next));
        else {
          out.delete("bbox");
          out.delete("project");
        }
        return out;
      },
      { replace: true },
    );
  }
  function search(e: FormEvent) {
    e.preventDefault();
    setFilters({
      q,
      organisation: org,
      year,
      status,
      has_overdue: overdue ? "true" : "",
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
          <FitToBbox bbox={fitBbox} />
          {fitBbox && (
            <Rectangle
              bounds={[[fitBbox[1], fitBbox[0]], [fitBbox[3], fitBbox[2]]]}
              pathOptions={{ color: "#0f766e", weight: 2, fillOpacity: 0.05 }}
            />
          )}
          <AreaButton onArea={setArea} />
        </MapContainer>
      </div>
      {bboxParam && !bbox && (
        <Banner tone="warning">The area in the link is not valid and was ignored.</Banner>
      )}
      {bbox && (
        <p className="mt-2 text-sm">
          Area filter active ({bboxKey}).{" "}
          <button
            type="button"
            className="text-teal-700 underline"
            onClick={() => setArea(null)}
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
                  <>
                    <Link
                      className="font-medium text-teal-700 underline"
                      to={`/app/projects/${r.id}`}
                    >
                      {r.title}
                    </Link>
                    {r.id === currentProject && (
                      <Badge tone="teal" className="ml-2">This project</Badge>
                    )}
                  </>
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
