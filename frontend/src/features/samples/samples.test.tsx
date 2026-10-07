import { render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, it, vi } from "vitest";
import type { SampleDto } from "../../api/types";
import { deliverableHref } from "../results/links";
import { LinkedSamples } from "./LinkedSamples";
import { SamplesTab } from "./SamplesTab";
import type * as UiModule from "../../ui";

function sample(
  code: string,
  related: { id: string; title: string }[],
): SampleDto {
  return {
    id: `s-${code}`,
    project_id: "p-1",
    code,
    site_id: null,
    collected_on: null,
    material: "seawater",
    custodian_org: "Te Moana University",
    storage_location: "MSB freezer 2",
    notes: "",
    related_deliverable_ids: related.map((d) => d.id),
    related_deliverables: related.map((d) => ({
      ...d,
      kind: "dataset",
      status: "accepted",
    })),
    created_at: "2026-01-01T10:00:00Z",
  };
}

const samples = [
  sample("WB-01", [{ id: "d-1", title: "Nutrient dataset" }]),
  sample("SD-02", []),
];

const deliverableSamples = vi.fn();

vi.mock("./api", () => ({
  useSamples: () => ({
    data: { items: samples, total: BigInt(samples.length) },
    isPending: false,
    isError: false,
  }),
  // Server-side filter (?deliverable_id=): the mock mimics it.
  useDeliverableSamples: (projectId: string, deliverableId: string) => {
    deliverableSamples(projectId, deliverableId);
    const items = samples.filter((s) =>
      s.related_deliverable_ids.includes(deliverableId),
    );
    return {
      data: { items, total: BigInt(items.length) },
      isPending: false,
      isError: false,
    };
  },
  useSampleAction: () => ({
    isPending: false,
    error: null,
    mutateAsync: vi.fn(),
  }),
}));
vi.mock("../projects/api", () => ({
  useProject: () => ({
    data: { application: { sites: [] }, results: { deliverables: [] } },
  }),
}));
// A viewer-role team member: may read, not edit.
vi.mock("../projects/projectContext", () => ({
  useProjectContext: () => ({
    project: { id: "p-1", my_access: "team_viewer" },
    me: { user: { id: "u-viewer", roles: [] } },
  }),
}));
vi.mock("../../ui", async (orig) => ({
  ...(await orig<typeof UiModule>()),
  useToast: () => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }),
}));

describe("sample ↔ result link", () => {
  it("SamplesTab shows related results linking to the deliverable for a viewer", () => {
    render(
      <MemoryRouter>
        <SamplesTab />
      </MemoryRouter>,
    );
    expect(screen.getAllByText("Related results").length).toBeGreaterThan(0);
    const links = screen.getAllByRole("link", { name: "Nutrient dataset" });
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) {
      expect(link.getAttribute("href")).toBe(deliverableHref("p-1", "d-1"));
    }
    expect(screen.queryByRole("button", { name: "Add sample" })).toBeNull();
  });

  it("deliverable detail lists linked samples with code, material and custodian", () => {
    render(<LinkedSamples projectId="p-1" deliverableId="d-1" />);
    expect(deliverableSamples).toHaveBeenCalledWith("p-1", "d-1");
    const section = screen.getByRole("region", { name: "Linked samples" });
    const items = within(section).getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toContain("WB-01");
    expect(items[0].textContent).toContain("seawater");
    expect(items[0].textContent).toContain("Te Moana University");
    expect(within(section).queryByText(/SD-02/)).toBeNull();
  });

  it("says so when no sample is linked", () => {
    render(<LinkedSamples projectId="p-1" deliverableId="d-9" />);
    expect(
      screen.getByText("No samples are linked to this result."),
    ).toBeTruthy();
  });
});
