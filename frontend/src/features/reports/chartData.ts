import type { MeasurementPointDto } from "../../api/types";
export function chartData(points: MeasurementPointDto[]) {
  const sites = Array.from(
    new Map(
      points.map((p) => [
        `${p.project_id}|${p.site}`,
        `${p.project_title} · ${p.site}`,
      ]),
    ).entries(),
  );
  const rows = new Map<string, Record<string, string | number>>();
  for (const p of points) {
    const key = `${p.project_id}|${p.site}`;
    const row: Record<string, string | number> = rows.get(p.observed_on) ?? {
      date: p.observed_on,
    };
    row[key] = p.value;
    row[`${key}:source`] = p.source_label;
    rows.set(p.observed_on, row);
  }
  return {
    sites,
    rows: Array.from(rows.values()).sort((a, b) =>
      String(a.date).localeCompare(String(b.date)),
    ),
  };
}
