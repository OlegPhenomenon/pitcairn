import { expect, it } from "vitest";
import type { MeasurementPointDto } from "../../api/types";
import { chartData } from "./chartData";
const point = (
  site: string,
  date: string,
  value: number,
): MeasurementPointDto => ({
  project_id: "p1",
  project_reference: "PIT-1",
  project_title: "Reef survey",
  site,
  observed_on: date,
  value,
  unit: "%",
  source_label: "survey.csv",
});
it("keeps each project and site as a separate chart series", () => {
  const result = chartData([
    point("A", "2026-01-01", 10),
    point("B", "2026-01-01", 30),
    point("A", "2026-01-02", 11),
  ]);
  expect(result.sites).toHaveLength(2);
  expect(result.rows).toEqual([
    {
      date: "2026-01-01",
      "p1|A": 10,
      "p1|A:source": "survey.csv",
      "p1|B": 30,
      "p1|B:source": "survey.csv",
    },
    { date: "2026-01-02", "p1|A": 11, "p1|A:source": "survey.csv" },
  ]);
});
