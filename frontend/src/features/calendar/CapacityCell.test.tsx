import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CapacityCell } from "./CapacityCell";
describe("CapacityCell", () => {
  it("shows free, partly used and full capacity and opens the selected day", () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <CapacityCell
        resource="Lab"
        date="2027-03-04"
        used={0n}
        capacity={2n}
        unit="benches"
        onClick={onClick}
      />,
    );
    const cell = screen.getByRole("button", {
      name: "Lab, 2027-03-04: 0 of 2 used",
    });
    expect(cell.className).toContain("bg-green-50");
    rerender(
      <CapacityCell
        resource="Lab"
        date="2027-03-04"
        used={1n}
        capacity={2n}
        unit="benches"
        onClick={onClick}
      />,
    );
    expect(screen.getByRole("button").className).toContain("bg-amber-100");
    rerender(
      <CapacityCell
        resource="Lab"
        date="2027-03-04"
        used={2n}
        capacity={2n}
        unit="benches"
        onClick={onClick}
      />,
    );
    expect(screen.getByRole("button").className).toContain("bg-red-100");
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledOnce();
  });
});
