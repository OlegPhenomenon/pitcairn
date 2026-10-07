import { useState } from "react";
import { parseErrorBody } from "../../api/client";
import { Button, useToast } from "../../ui";
import { useProjectContext } from "../projects/ProjectLayout";
import { CloseDialog } from "./ResultsTab";
export function ProjectTools() {
  const { project, me } = useProjectContext();
  const allowed = Boolean(
    me?.user.roles.some((r) => ["coordinator", "admin"].includes(r)),
  );
  const coordinator = Boolean(me?.user.roles.includes("coordinator"));
  const [close, setClose] = useState(false),
    [busy, setBusy] = useState(false);
  const toast = useToast();
  if (!allowed) return null;
  async function download() {
    setBusy(true);
    try {
      const response = await fetch(`/api/v1/projects/${project.id}/export`, {
        credentials: "include",
      });
      if (!response.ok)
        throw parseErrorBody(response.status, await response.json());
      const blob = await response.blob();
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${project.reference ?? project.id}.zip`;
      link.click();
      URL.revokeObjectURL(url);
      toast.success("Project export downloaded");
    } catch (e) {
      toast.error(
        "Could not export project",
        e instanceof Error ? e.message : undefined,
      );
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="mt-4 flex flex-wrap gap-2">
      <Button
        size="sm"
        variant="secondary"
        disabled={busy}
        onClick={() => void download()}
      >
        {busy ? "Preparing export…" : "Export project ZIP"}
      </Button>
      {coordinator && project.status === "approved" && (
        <Button size="sm" variant="secondary" onClick={() => setClose(true)}>
          Close project
        </Button>
      )}
      <CloseDialog
        open={close}
        onClose={() => setClose(false)}
        projectId={project.id}
      />
    </div>
  );
}
