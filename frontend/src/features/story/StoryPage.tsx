import { useState } from "react";
import { useNavigate } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { apiGet } from "../../api/client";
import type { ListResponse, ProjectListItemDto } from "../../api/types";
import { Banner, Button, Card, CardBody, PageHeader, useToast } from "../../ui";
import { useMe } from "../auth/api";
import { useDemoSwitch } from "../demo/api";
const steps = [
  {
    title: "Submit the application",
    persona: "anna",
    name: "Anna",
    tab: "application",
    body: "Anna completes the form and sends it to Pitcairn.",
  },
  {
    title: "Request a missing document",
    persona: "maria",
    name: "Maria",
    tab: "messages",
    body: "Maria asks Anna for the document needed to review the application.",
  },
  {
    title: "Get an expert view",
    persona: "james",
    name: "James",
    tab: "review",
    body: "The assigned expert reviews the research proposal.",
  },
  {
    title: "Issue the decision",
    persona: "helen",
    name: "Helen",
    tab: "decisions",
    body: "The decision maker issues the permit with its conditions.",
  },
  {
    title: "Plan a trip and bookings",
    persona: "anna",
    name: "Anna",
    tab: "trips",
    body: "Anna requests base space and transport for fieldwork.",
  },
  {
    title: "Agree what must be delivered",
    persona: "maria",
    name: "Maria",
    tab: "results",
    body: "Maria proposes the reports and data Pitcairn expects.",
  },
  {
    title: "Change the agreement",
    persona: "anna",
    name: "Anna",
    tab: "results",
    body: "Anna proposes a due date change with a reason; Maria must agree again.",
  },
  {
    title: "Submit and return results",
    persona: "anna",
    name: "Anna",
    tab: "results",
    body: "Anna submits results. Maria can return one submission for correction.",
  },
  {
    title: "Accept and publish",
    persona: "maria",
    name: "Maria",
    tab: "results",
    body: "Maria accepts receipt and chooses what appears in the public catalog.",
  },
];
export function StoryPage() {
  const me = useMe(),
    switcher = useDemoSwitch(),
    cache = useQueryClient(),
    navigate = useNavigate(),
    toast = useToast();
  const [busy, setBusy] = useState<number | null>(null);
  async function go(i: number) {
    const step = steps[i];
    setBusy(i);
    try {
      await switcher.mutateAsync({ persona_key: step.persona });
      cache.clear();
      const projects = await apiGet<ListResponse<ProjectListItemDto>>(
        "/projects?limit=100",
      );
      const coral = projects.items.find((p) =>
        p.title.toLowerCase().includes("coral health"),
      );
      const saved = sessionStorage.getItem("pitcairn-story-project");
      const id = coral?.id ?? saved ?? projects.items[0]?.id;
      if (coral) sessionStorage.setItem("pitcairn-story-project", coral.id);
      navigate(id ? `/app/projects/${id}/${step.tab}` : "/app");
      toast.success(`Switched to ${step.name}`);
    } catch (e) {
      toast.error(
        "Could not open this step",
        e instanceof Error ? e.message : undefined,
      );
    } finally {
      setBusy(null);
    }
  }
  if (!me.data?.demo_mode)
    return (
      <Banner tone="error">The story guide is available in demo mode.</Banner>
    );
  return (
    <>
      <PageHeader
        title="Anna and Maria: a research story"
        subtitle="Walk through the permit, fieldwork and results in nine steps."
      />
      <ol className="grid gap-4 md:grid-cols-2">
        {steps.map((step, i) => (
          <li key={step.title}>
            <Card className="h-full">
              <CardBody>
                <p className="text-xs font-semibold text-teal-700">
                  STEP {i + 1} OF 9
                </p>
                <h2 className="mt-1 text-lg font-semibold">{step.title}</h2>
                <p className="mt-1 text-sm text-slate-600">{step.body}</p>
                <Button
                  className="mt-4"
                  disabled={busy !== null}
                  onClick={() => void go(i)}
                >
                  {busy === i
                    ? "Switching…"
                    : `Switch to ${step.name} and open`}
                </Button>
              </CardBody>
            </Card>
          </li>
        ))}
      </ol>
      <p className="mt-6 text-sm">
        <a href="/catalog" className="text-teal-700 underline">
          Open the public catalog
        </a>{" "}
        to see published results.
      </p>
    </>
  );
}
