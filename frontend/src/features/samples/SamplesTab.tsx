import { useState, type FormEvent } from "react";
import { fieldError } from "../../api/client";
import type { CreateSampleRequest, SampleDto } from "../../api/types";
import { formatDate } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  Dialog,
  FormField,
  Input,
  Select,
  Skeleton,
  Table,
  Textarea,
  useToast,
} from "../../ui";
import { useProject } from "../projects/api";
import { useProjectContext } from "../projects/projectContext";
import { useSampleAction, useSamples } from "./api";
export function SamplesTab() {
  const { project, me } = useProjectContext();
  const list = useSamples(project.id),
    workspace = useProject(project.id);
  const action = useSampleAction(project.id),
    toast = useToast();
  const canEdit =
    ["team_editor", "team_lead"].includes(project.my_access) ||
    Boolean(me?.user.roles.includes("coordinator"));
  const [item, setItem] = useState<SampleDto | null>(null),
    [open, setOpen] = useState(false),
    [code, setCode] = useState(""),
    [site, setSite] = useState(""),
    [date, setDate] = useState(""),
    [material, setMaterial] = useState(""),
    [custodian, setCustodian] = useState(""),
    [storage, setStorage] = useState(""),
    [notes, setNotes] = useState(""),
    [related, setRelated] = useState<string[]>([]),
    [confirm, setConfirm] = useState(false);
  const edit = (s: SampleDto | null) => {
    setItem(s);
    setCode(s?.code ?? "");
    setSite(s?.site_id ?? "");
    setDate(s?.collected_on ?? "");
    setMaterial(s?.material ?? "");
    setCustodian(s?.custodian_org ?? "");
    setStorage(s?.storage_location ?? "");
    setNotes(s?.notes ?? "");
    setRelated(s?.related_deliverable_ids ?? []);
    setOpen(true);
  };
  async function save(e: FormEvent) {
    e.preventDefault();
    const body: CreateSampleRequest = {
      code,
      site_id: site || null,
      collected_on: date || null,
      material,
      custodian_org: custodian,
      storage_location: storage,
      notes,
      related_deliverable_ids: related,
    };
    try {
      await action.mutateAsync({
        kind: item ? "update" : "create",
        id: item?.id,
        body,
      });
      toast.success(item ? "Sample updated" : "Sample recorded");
      setOpen(false);
    } catch {
      /* inline */
    }
  }
  async function remove() {
    try {
      await action.mutateAsync({ kind: "delete", id: item?.id });
      toast.success("Sample deleted");
      setConfirm(false);
      setOpen(false);
    } catch {
      /* hook toast */
    }
  }
  return (
    <Card>
      <CardHeader
        title="Samples"
        actions={
          canEdit && (
            <Button size="sm" onClick={() => edit(null)}>
              Add sample
            </Button>
          )
        }
      />
      <CardBody>
        {list.isError && <Banner tone="error">{list.error.message}</Banner>}
        {list.isPending ? (
          <Skeleton className="h-32" />
        ) : (
          <Table
            rows={list.data?.items ?? []}
            rowKey={(s) => s.id}
            empty={{
              title: "No samples recorded",
              body: "Add a sample to record where it is held and who looks after it.",
            }}
            columns={[
              {
                header: "Code",
                cell: (s) =>
                  canEdit ? (
                    <button
                      className="font-medium text-teal-700 underline"
                      onClick={() => edit(s)}
                    >
                      {s.code}
                    </button>
                  ) : (
                    s.code
                  ),
              },
              {
                header: "Site",
                cell: (s) =>
                  workspace.data?.application.sites.find(
                    (x) => x.id === s.site_id,
                  )?.name ?? "—",
              },
              {
                header: "Collected",
                cell: (s) => (
                  <time title={s.collected_on ?? ""}>
                    {formatDate(s.collected_on)}
                  </time>
                ),
              },
              { header: "Material", cell: (s) => s.material },
              { header: "Custodian", cell: (s) => s.custodian_org },
              { header: "Storage", cell: (s) => s.storage_location },
            ]}
          />
        )}
      </CardBody>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title={item ? "Edit sample" : "Add sample"}
        footer={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            {item && (
              <Button variant="secondary" onClick={() => setConfirm(true)}>
                Delete
              </Button>
            )}
            <Button
              type="submit"
              form="sample-form"
              disabled={action.isPending}
            >
              {action.isPending ? "Saving…" : "Save sample"}
            </Button>
          </>
        }
      >
        <form
          id="sample-form"
          onSubmit={(e) => void save(e)}
          className="space-y-3"
        >
          <FormField
            label="Code"
            required
            error={fieldError(action.error, "code")}
          >
            <Input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              required
            />
          </FormField>
          <FormField label="Site" error={fieldError(action.error, "site_id")}>
            <Select value={site} onChange={(e) => setSite(e.target.value)}>
              <option value="">No linked site</option>
              {workspace.data?.application.sites.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField
            label="Collected on"
            error={fieldError(action.error, "collected_on")}
          >
            <Input
              type="date"
              value={date}
              onChange={(e) => setDate(e.target.value)}
            />
          </FormField>
          {[
            ["Material", material, setMaterial],
            ["Custodian organisation", custodian, setCustodian],
            ["Storage location", storage, setStorage],
          ].map(([label, value, set]) => (
            <FormField key={label as string} label={label as string}>
              <Input
                value={value as string}
                onChange={(e) => (set as (v: string) => void)(e.target.value)}
              />
            </FormField>
          ))}
          <fieldset>
            <legend className="text-sm font-medium">
              Related deliverables
            </legend>
            {workspace.data?.results.deliverables.map((d) => (
              <label key={d.id} className="flex gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={related.includes(d.id)}
                  onChange={(e) =>
                    setRelated((v) =>
                      e.target.checked
                        ? [...v, d.id]
                        : v.filter((x) => x !== d.id),
                    )
                  }
                />
                {d.title}
              </label>
            ))}
          </fieldset>
          <FormField label="Notes">
            <Textarea
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
          </FormField>
          {action.error && <Banner tone="error">{action.error.message}</Banner>}
        </form>
      </Dialog>
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Delete sample"
        footer={
          <>
            <Button variant="secondary" onClick={() => setConfirm(false)}>
              Keep sample
            </Button>
            <Button disabled={action.isPending} onClick={() => void remove()}>
              Delete sample
            </Button>
          </>
        }
      >
        <p>This removes the sample record from this project.</p>
      </Dialog>
    </Card>
  );
}
