import { Link } from "react-router";
import { Plus } from "lucide-react";
import { useApiQuery } from "../../api/client";
import type { DashboardResponse } from "../../api/types";
import { formatDate } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  PageHeader,
  Skeleton,
  StatusBadge,
} from "../../ui";
import { useMe } from "../auth/api";
import { moneyText } from "./display";
const labels: Record<string, string> = {
  my_projects: "My research",
  needs_reply: "Needs your reply",
  decisions: "Pitcairn decisions",
  trips_and_invoices: "Trips & invoices",
  results_due: "Deliver results",
  new_applications: "New applications",
  waiting_for_applicant: "Waiting for the applicant",
  with_experts: "With experts",
  arriving_soon: "Arriving soon",
  results_to_check: "Results to check",
  overdue_results: "Overdue results",
  assigned_reviews: "Assigned reviews",
  pending_bookings: "Booking requests",
  arrivals: "Arrivals",
  invoices_to_issue: "Invoices to issue",
  payments_to_verify: "Payments to verify",
  my_requests: "My requests",
};
const order = Object.keys(labels);
export function DashboardPage() {
  const me = useMe(),
    data = useApiQuery<DashboardResponse>(["dashboard"], "/dashboard");
  const roles = me.data?.user.roles ?? [];
  const canCreate =
    !roles.some((r) => ["provider", "expert"].includes(r)) ||
    roles.includes("coordinator");
  return (
    <>
      <PageHeader
        title={`Welcome, ${me.data?.user.name ?? ""}`}
        subtitle="Your work and what needs attention."
        actions={
          canCreate && (
            <Link to="/app/projects/new">
              <Button icon={<Plus className="size-4" />}>New project</Button>
            </Link>
          )
        }
      />
      {data.isError && <Banner tone="error">{data.error.message}</Banner>}
      {data.isPending ? (
        <div className="grid gap-4 sm:grid-cols-2">
          <Skeleton className="h-48" />
          <Skeleton className="h-48" />
        </div>
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          {order
            .filter((key) => Object.hasOwn(data.data?.sections ?? {}, key))
            .map((key) => {
              const items = data.data?.sections[key] ?? [];
              return (
                <Card key={key}>
                  <CardHeader title={`${labels[key]} (${items.length})`} />
                  <CardBody>
                    {items.length === 0 ? (
                      <EmptyState
                        title="All caught up"
                        body={`Nothing in ${labels[key].toLowerCase()} right now.`}
                      />
                    ) : (
                      <ul className="divide-y divide-slate-100">
                        {items.map((item, i) => (
                          <li
                            key={`${item.link}-${i}`}
                            className="py-3 first:pt-0 last:pb-0"
                          >
                            <Link
                              to={item.link}
                              className="font-medium text-teal-800 hover:underline"
                            >
                              {moneyText(item.title)}
                            </Link>
                            <p className="text-sm text-slate-600">
                              {item.kind === "project" &&
                              item.subtitle.startsWith("status: ") ? (
                                <StatusBadge
                                  status={item.subtitle.slice(8)}
                                  label={
                                    item.subtitle.slice(8) === "approved"
                                      ? "Permit approved"
                                      : undefined
                                  }
                                />
                              ) : (
                                moneyText(item.subtitle)
                              )}
                            </p>
                            <div className="mt-1 flex gap-2 text-xs text-slate-500">
                              {item.project_reference && (
                                <span>{item.project_reference}</span>
                              )}
                              {item.due_date && (
                                <time
                                  dateTime={item.due_date}
                                  title={item.due_date}
                                >
                                  Due {formatDate(item.due_date)}
                                </time>
                              )}
                              {item.kind === "decision" && (
                                <StatusBadge
                                  status="issued"
                                  label="Decision issued"
                                />
                              )}
                            </div>
                          </li>
                        ))}
                      </ul>
                    )}
                  </CardBody>
                </Card>
              );
            })}
          {data.data && Object.keys(data.data.sections).length === 0 && (
            <EmptyState
              title="No dashboard sections"
              body="There is no work assigned to this account yet."
            />
          )}
        </div>
      )}
    </>
  );
}
