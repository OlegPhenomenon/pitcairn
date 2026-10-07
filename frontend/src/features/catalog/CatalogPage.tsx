import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import { Anchor } from "lucide-react";
import { MapContainer, Marker, Polygon, Popup, TileLayer } from "react-leaflet";
import '../../lib/leafletIcons';
import { useApiQuery, listQuery } from "../../api/client";
import type { ListResponse, PublicProjectDto } from "../../api/types";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  EmptyState,
  FormField,
  Input,
  Select,
  Skeleton,
} from "../../ui";
export function CatalogPage() {
  const [text, setText] = useState(""),
    [year, setYear] = useState(""),
    [filters, setFilters] = useState({ q: "", year: "" });
  const list = useApiQuery<ListResponse<PublicProjectDto>>(
    ["catalog", filters],
    `/public/projects${listQuery(100, 0, filters)}`,
  );
  const items = list.data?.items ?? [];
  const years = Array.from({ length: 20 }, (_, i) =>
    String(new Date().getFullYear() - i),
  );
  function submit(e: FormEvent) {
    e.preventDefault();
    setFilters({ q: text, year });
  }
  return (
    <div className="flex min-h-dvh flex-col bg-sand-50">
      <header className="bg-navy-900 text-white">
        <nav className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
          <Link to="/" className="inline-flex items-center gap-2 font-semibold">
            <Anchor className="size-5 text-teal-300" />
            Pitcairn Research Hub
          </Link>
          <Link to="/login" className="text-sm underline">
            Log in
          </Link>
        </nav>
      </header>
      <main id="main" className="mx-auto w-full max-w-6xl flex-1 px-4 py-8">
        <h1 className="text-3xl font-bold text-navy-900">Open catalog</h1>
        <p className="mt-1 text-slate-600">
          Published results from research around the Pitcairn Islands.
        </p>
        <form
          onSubmit={submit}
          className="mt-6 grid gap-3 rounded-lg border bg-white p-4 sm:grid-cols-[1fr_10rem_auto] sm:items-end"
        >
          <FormField label="Search published research">
            <Input
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="Topic, project, organisation"
            />
          </FormField>
          <FormField label="Year">
            <Select value={year} onChange={(e) => setYear(e.target.value)}>
              <option value="">All years</option>
              {years.map((y) => (
                <option key={y}>{y}</option>
              ))}
            </Select>
          </FormField>
          <Button type="submit">Search</Button>
        </form>
        <div className="mt-6 grid gap-6 lg:grid-cols-[1fr_2fr]">
          <div className="overflow-hidden rounded-lg border bg-white">
            <MapContainer
              center={[-25.0667, -130.1]}
              zoom={7}
              scrollWheelZoom={false}
              className="h-72 w-full"
            >
              <TileLayer url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png" />
              {items.flatMap((p) =>
                p.sites.map((s, i) => {
                  const g = s.geometry as {
                    type?: string;
                    coordinates?: unknown;
                  };
                  let point: [number, number] | null = null;
                  if (g?.type === "Point" && Array.isArray(g.coordinates))
                    point = [
                      Number(g.coordinates[1]),
                      Number(g.coordinates[0]),
                    ];
                  if (g?.type === "Polygon" && Array.isArray(g.coordinates)) {
                    const ring = g.coordinates[0] as number[][];
                    return ring?.length ? (
                      <Polygon
                        key={`${p.reference}-${i}`}
                        positions={ring.map(
                          (c) => [c[1], c[0]] as [number, number],
                        )}
                        pathOptions={{ color: "#0f766e" }}
                      >
                        <Popup>
                          <Link to={`/catalog/${p.reference}`}>{p.title}</Link>
                          <br />
                          {s.name}
                          {s.generalized && " · generalized area"}
                        </Popup>
                      </Polygon>
                    ) : null;
                  }
                  return point ? (
                    <Marker key={`${p.reference}-${i}`} position={point}>
                      <Popup>
                        <Link to={`/catalog/${p.reference}`}>{p.title}</Link>
                        <br />
                        {s.name}
                      </Popup>
                    </Marker>
                  ) : null;
                }),
              )}
            </MapContainer>
            <p className="p-2 text-xs text-slate-500">
              Sites with sensitive locations appear as generalized areas. ©
              OpenStreetMap contributors
            </p>
          </div>
          <section aria-label="Catalog results">
            {list.isPending ? (
              <div className="space-y-3">
                <Skeleton className="h-28" />
                <Skeleton className="h-28" />
              </div>
            ) : list.isError ? (
              <Banner tone="error">{list.error.message}</Banner>
            ) : items.length === 0 ? (
              <EmptyState
                title="No published projects found"
                body="Try a different search or year."
              />
            ) : (
              <>
                <p className="mb-3 text-sm text-slate-500">
                  {toNum(list.data?.total ?? 0)} published project(s)
                </p>
                <ul className="space-y-3">
                  {items
                    .filter((p) => p.reference)
                    .map((p) => (
                      <li
                        key={p.reference}
                        className="rounded-lg border bg-white p-4 shadow-sm"
                      >
                        <Link
                          to={`/catalog/${p.reference}`}
                          className="font-semibold text-teal-800 underline"
                        >
                          {p.title}
                        </Link>
                        <p className="text-sm text-slate-600">
                          {p.reference} · {p.organisation} ·{" "}
                          {p.year ? toNum(p.year) : "Year unavailable"}
                        </p>
                        <p className="mt-2 line-clamp-2 text-sm">{p.summary}</p>
                        <p className="mt-2 text-xs text-slate-500">
                          {p.deliverables.length} published deliverable(s)
                        </p>
                      </li>
                    ))}
                </ul>
              </>
            )}
          </section>
        </div>
      </main>
      <footer className="border-t p-4 text-center text-xs text-slate-600">
        Only material chosen for publication appears in this catalog.
      </footer>
    </div>
  );
}
