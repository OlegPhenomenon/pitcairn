import { useState } from "react";
import { Link } from "react-router";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { listQuery, useApiQuery } from "../../api/client";
import type {
  DeliverablesReportResponse,
  ListResponse,
  MeasurementVariableDto,
  MeasurementsReportResponse,
} from "../../api/types";
import { formatDate, toNum } from "../../lib/format";
import { chartData } from "./chartData";
import {
  Banner,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  FormField,
  PageHeader,
  Select,
  Skeleton,
  Table,
} from "../../ui";
const n = (x: bigint) => toNum(x);
const colors = [
  "#0f766e",
  "#1d4ed8",
  "#b45309",
  "#7c3aed",
  "#be123c",
  "#047857",
];
export function ReportsPage() {
  const [variable, setVariable] = useState("");
  const totals = useApiQuery<DeliverablesReportResponse>(
    ["reports", "deliverables"],
    "/reports/deliverables",
  );
  const variables = useApiQuery<ListResponse<MeasurementVariableDto>>(
    ["reports", "variables"],
    "/reports/measurements/variables",
  );
  const measurements = useApiQuery<MeasurementsReportResponse>(
    ["reports", "measurements", variable],
    `/reports/measurements${listQuery(undefined, undefined, { variable_key: variable })}`,
    { enabled: !!variable },
  );
  return (
    <>
      <PageHeader
        title="Reports"
        subtitle="Results delivery and published measurements."
      />
      <Card>
        <CardHeader title="Deliverables by project" />
        <CardBody>
          {totals.isError && (
            <Banner tone="error">{totals.error.message}</Banner>
          )}
          <Table
            loading={totals.isPending}
            rows={totals.data?.projects ?? []}
            rowKey={(r) => r.project_id}
            empty={{ title: "No deliverables to report" }}
            columns={[
              {
                header: "Project",
                cell: (r) => (
                  <Link
                    to={`/app/projects/${r.project_id}/results`}
                    className="text-teal-700 underline"
                  >
                    {r.project_reference ?? r.project_title}
                  </Link>
                ),
              },
              { header: "Organisation", cell: (r) => r.organisation },
              { header: "Agreed", cell: (r) => n(r.agreed) },
              { header: "Received", cell: (r) => n(r.received) },
              { header: "Accepted", cell: (r) => n(r.accepted) },
              { header: "Overdue", cell: (r) => n(r.overdue) },
            ]}
          />
        </CardBody>
      </Card>
      <div className="mt-5 grid gap-5 lg:grid-cols-2">
        {(["totals_by_year", "totals_by_organisation"] as const).map((key) => (
          <Card key={key}>
            <CardHeader
              title={
                key === "totals_by_year"
                  ? "Totals by year"
                  : "Totals by organisation"
              }
            />
            <CardBody>
              <Table
                loading={totals.isPending}
                rows={totals.data?.[key] ?? []}
                rowKey={(r) => r.key}
                empty={{ title: "No totals yet" }}
                columns={[
                  {
                    header: key === "totals_by_year" ? "Year" : "Organisation",
                    cell: (r) => r.key,
                  },
                  { header: "Agreed", cell: (r) => n(r.agreed) },
                  { header: "Received", cell: (r) => n(r.received) },
                  { header: "Accepted", cell: (r) => n(r.accepted) },
                  { header: "Overdue", cell: (r) => n(r.overdue) },
                ]}
              />
            </CardBody>
          </Card>
        ))}
      </div>
      <Card className="mt-5">
        <CardHeader title="Measurements" />
        <CardBody>
          <FormField label="Variable">
            <Select
              value={variable}
              onChange={(e) => setVariable(e.target.value)}
            >
              <option value="">Choose a variable</option>
              {Array.from(
                new Set(variables.data?.items.map((v) => v.variable_key) ?? []),
              ).map((v) => (
                <option key={v}>{v}</option>
              ))}
            </Select>
          </FormField>
          {variables.isPending && <Skeleton className="mt-3 h-10" />}
          {variables.isError && (
            <Banner tone="error">{variables.error.message}</Banner>
          )}
          {measurements.isPending && variable && (
            <Skeleton className="mt-4 h-64" />
          )}
          {measurements.isError && (
            <Banner tone="error">{measurements.error.message}</Banner>
          )}
          {measurements.data && measurements.data.series.length === 0 && (
            <EmptyState title="No measurements for this variable" />
          )}
          {measurements.data?.series.map((series) => (
            <section key={series.unit} className="mt-5">
              <h3 className="font-semibold">
                {variable} ({series.unit})
              </h3>
              <div className="h-64 min-w-0">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={chartData(series.points).rows}>
                    <CartesianGrid strokeDasharray="3 3" />
                    <XAxis dataKey="date" />
                    <YAxis
                      label={{
                        value: series.unit,
                        angle: -90,
                        position: "insideLeft",
                      }}
                    />
                    <Tooltip
                      content={({ label, payload }) => (
                        <div className="rounded border bg-white p-2 text-sm">
                          <strong>{formatDate(String(label))}</strong>
                          {payload?.map((entry, i) => (
                            <p key={i}>
                              {entry.name}: {String(entry.value)} {series.unit}
                              <br />
                              Source:{" "}
                              {String(
                                (entry.payload as Record<string, unknown>)[
                                  `${entry.dataKey}:source`
                                ] ?? "—",
                              )}
                            </p>
                          ))}
                        </div>
                      )}
                    />
                    <Legend />
                    {chartData(series.points).sites.map(([key, name], i) => (
                      <Line
                        key={key}
                        type="linear"
                        dataKey={key}
                        name={name}
                        stroke={colors[i % colors.length]}
                        connectNulls
                        dot
                      />
                    ))}
                  </LineChart>
                </ResponsiveContainer>
              </div>
              <Table
                rows={series.points}
                rowKey={(p) =>
                  `${p.project_id}-${p.site}-${p.observed_on}-${p.source_label}`
                }
                empty={{ title: "No measurements" }}
                columns={[
                  {
                    header: "Date",
                    cell: (p) => (
                      <time title={p.observed_on}>
                        {formatDate(p.observed_on)}
                      </time>
                    ),
                  },
                  {
                    header: "Project",
                    cell: (p) => (
                      <Link
                        className="text-teal-700 underline"
                        to={`/app/projects/${p.project_id}`}
                      >
                        {p.project_reference ?? p.project_title}
                      </Link>
                    ),
                  },
                  { header: "Site", cell: (p) => p.site },
                  { header: "Value", cell: (p) => `${p.value} ${p.unit}` },
                  { header: "Source", cell: (p) => p.source_label },
                ]}
              />
            </section>
          ))}
        </CardBody>
      </Card>
    </>
  );
}
