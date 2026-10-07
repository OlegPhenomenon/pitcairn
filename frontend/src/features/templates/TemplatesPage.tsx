import { useState } from "react";
import {
  apiPost,
  apiPut,
  fieldError,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type {
  ListResponse,
  TemplateDto,
  TemplateSchemaRequest,
  TemplateVersionDto,
} from "../../api/types";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  Checkbox,
  Dialog,
  EmptyState,
  FormField,
  Input,
  PageHeader,
  Select,
  Skeleton,
  StatusBadge,
  Textarea,
  useToast,
} from "../../ui";
const palette = [
  "text",
  "textarea",
  "date",
  "daterange",
  "number",
  "select",
  "multiselect",
  "people",
  "checkbox",
  "sites",
];
type Field = {
  key: string;
  label: string;
  type: string;
  help: string;
  required: boolean;
  options: string[];
};
type Section = { key: string; title: string; help: string; fields: Field[] };
type Document = { key: string; label: string; help: string; category: string };
type Schema = { sections: Section[]; required_documents: Document[] };
const makeField = (): Field => ({
  key: "",
  label: "",
  type: "text",
  help: "",
  required: false,
  options: [],
});
const makeSection = (): Section => ({
  key: "",
  title: "",
  help: "",
  fields: [],
});
const makeDocument = (): Document => ({
  key: "",
  label: "",
  help: "",
  category: "application",
});
function fromSchema(raw: Record<string, unknown>): Schema {
  const s = raw as Partial<Schema>;
  return {
    sections: Array.isArray(s.sections) ? s.sections : [],
    required_documents: Array.isArray(s.required_documents)
      ? s.required_documents
      : [],
  };
}
export function TemplatesPage() {
  const templates = useApiQuery<ListResponse<TemplateDto>>(
    ["templates"],
    "/templates",
  );
  const [key, setKey] = useState("");
  const versions = useApiQuery<ListResponse<TemplateVersionDto>>(
    ["templates", key, "versions"],
    `/templates/${key}/versions`,
    { enabled: !!key },
  );
  const [editing, setEditing] = useState<TemplateVersionDto | null>(null);
  const [schema, setSchema] = useState<Schema>({
    sections: [],
    required_documents: [],
  });
  const [preview, setPreview] = useState(false),
    [confirm, setConfirm] = useState(false);
  const toast = useToast();
  const create = useApiMutation<TemplateVersionDto, TemplateSchemaRequest>(
    (body) => apiPost(`/templates/${key}/versions`, body),
    { invalidate: [["templates", key, "versions"], ["templates"]] },
  );
  const save = useApiMutation<TemplateVersionDto, TemplateSchemaRequest>(
    (body) => apiPut(`/template-versions/${editing?.id}`, body),
    { invalidate: [["templates", key, "versions"]] },
  );
  const publish = useApiMutation<TemplateVersionDto, void>(
    () => apiPost(`/template-versions/${editing?.id}/publish`),
    { invalidate: [["templates", key, "versions"], ["templates"]] },
  );
  const err = create.error ?? save.error;
  function open(v: TemplateVersionDto) {
    setEditing(v);
    setSchema(fromSchema(v.schema));
    setPreview(false);
  }
  async function draft() {
    const latest = versions.data?.items[0];
    if (!latest) return;
    try {
      const v = await create.mutateAsync({ schema: latest.schema });
      toast.success("Draft version created");
      open(v);
    } catch {
      /* hook toast */
    }
  }
  async function persist() {
    try {
      const v = await save.mutateAsync({ schema });
      toast.success("Draft saved");
      setEditing(v);
    } catch {
      /* show fields */
    }
  }
  async function doPublish() {
    try {
      await publish.mutateAsync();
      toast.success("Template published");
      setConfirm(false);
      setEditing(null);
    } catch {
      /* hook toast */
    }
  }
  const updateSection = (i: number, patch: Partial<Section>) =>
    setSchema((s) => ({
      ...s,
      sections: s.sections.map((x, j) => (j === i ? { ...x, ...patch } : x)),
    }));
  const updateField = (i: number, j: number, patch: Partial<Field>) =>
    setSchema((s) => ({
      ...s,
      sections: s.sections.map((x, a) =>
        a === i
          ? {
              ...x,
              fields: x.fields.map((f, b) =>
                b === j ? { ...f, ...patch } : f,
              ),
            }
          : x,
      ),
    }));
  return (
    <>
      <PageHeader
        title="Application templates"
        subtitle="Published versions apply to new projects."
      />
      {templates.isError && (
        <Banner tone="error">{templates.error.message}</Banner>
      )}
      {templates.isPending ? (
        <Skeleton className="h-32" />
      ) : (
        <div className="grid gap-4 md:grid-cols-3">
          {templates.data?.items.length === 0 && (
            <EmptyState title="No templates yet" body="Create a template in administration to begin." />
          )}
          {templates.data?.items.map((t) => (
            <button
              key={t.key}
              onClick={() => {
                setKey(t.key);
                setEditing(null);
              }}
              className={`rounded-lg border bg-white p-4 text-left ${key === t.key ? "border-teal-600" : "border-slate-200"}`}
            >
              <strong>{t.name}</strong>
              <p className="text-sm text-slate-600">{t.description}</p>
              <p className="mt-2 text-xs">
                Published v
                {t.latest_published_version
                  ? toNum(t.latest_published_version)
                  : "—"}{" "}
                · Draft v{t.draft_version ? toNum(t.draft_version) : "—"}
              </p>
            </button>
          ))}
        </div>
      )}
      {key && (
        <Card className="mt-5">
          <CardHeader
            title="Versions"
            actions={
              <Button
                size="sm"
                disabled={!versions.data?.items.length || create.isPending}
                onClick={() => void draft()}
              >
                {create.isPending ? "Creating…" : "New draft from latest"}
              </Button>
            }
          />
          <CardBody>
            {versions.isError && (
              <Banner tone="error">{versions.error.message}</Banner>
            )}
            {versions.isPending ? (
              <Skeleton className="h-24" />
            ) : (
              <div className="space-y-2">
                {versions.data?.items.length === 0 && (
                  <EmptyState title="No versions yet" />
                )}
                {versions.data?.items.map((v) => (
                  <button
                    key={v.id}
                    onClick={() => open(v)}
                    className="flex w-full items-center justify-between rounded border p-3 text-left hover:bg-slate-50"
                  >
                    <span>Version {toNum(v.version)}</span>
                    <StatusBadge status={v.status} />
                  </button>
                ))}
              </div>
            )}
          </CardBody>
        </Card>
      )}
      {editing && (
        <Card className="mt-5">
          <CardHeader
            title={`Version ${toNum(editing.version)}`}
            actions={
              <div className="flex gap-2">
                <StatusBadge status={editing.status} />
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setPreview((v) => !v)}
                >
                  {preview ? "Edit" : "Preview"}
                </Button>
                {editing.status === "draft" && (
                  <Button size="sm" onClick={() => setConfirm(true)}>
                    Publish
                  </Button>
                )}
              </div>
            }
          />
          <CardBody>
            {preview ? (
              <div className="space-y-5">
                {schema.sections.map((section, i) => (
                  <section key={i}>
                    <h3 className="font-semibold">{section.title}</h3>
                    <p className="text-sm">{section.help}</p>
                    <div className="mt-3 grid gap-3">
                      {section.fields.map((f, j) => (
                        <FormField
                          key={j}
                          label={f.label}
                          required={f.required}
                          help={f.help}
                        >
                          {f.type === "textarea" ? (
                            <Textarea disabled />
                          ) : f.type === "select" ||
                            f.type === "multiselect" ? (
                            <Select disabled>
                              <option>Choose an option</option>
                              {f.options.map((x) => (
                                <option key={x}>{x}</option>
                              ))}
                            </Select>
                          ) : (
                            <Input
                              disabled
                              type={
                                f.type === "date"
                                  ? "date"
                                  : f.type === "number"
                                    ? "number"
                                    : "text"
                              }
                            />
                          )}
                        </FormField>
                      ))}
                    </div>
                  </section>
                ))}
                <h3 className="font-semibold">Required documents</h3>
                {schema.required_documents.map((d, i) => (
                  <p key={i}>{d.label}</p>
                ))}
              </div>
            ) : editing.status === "draft" ? (
              <div className="space-y-5">
                {schema.sections.map((section, i) => (
                  <section key={i} className="rounded border p-3">
                    <div className="grid gap-3 sm:grid-cols-3">
                      <FormField
                        label="Section key"
                        error={fieldError(err, `sections[${i}].key`)}
                      >
                        <Input
                          value={section.key}
                          onChange={(e) =>
                            updateSection(i, { key: e.target.value })
                          }
                        />
                      </FormField>
                      <FormField
                        label="Title"
                        error={fieldError(err, `sections[${i}].title`)}
                      >
                        <Input
                          value={section.title}
                          onChange={(e) =>
                            updateSection(i, { title: e.target.value })
                          }
                        />
                      </FormField>
                      <FormField label="Help">
                        <Input
                          value={section.help}
                          onChange={(e) =>
                            updateSection(i, { help: e.target.value })
                          }
                        />
                      </FormField>
                    </div>
                    {section.fields.map((f, j) => (
                      <div key={j} className="mt-3 rounded bg-slate-50 p-3">
                        <div className="grid gap-3 sm:grid-cols-3">
                          <FormField
                            label="Field key"
                            error={fieldError(
                              err,
                              `sections[${i}].fields[${j}].key`,
                            )}
                          >
                            <Input
                              value={f.key}
                              onChange={(e) =>
                                updateField(i, j, { key: e.target.value })
                              }
                            />
                          </FormField>
                          <FormField
                            label="Label"
                            error={fieldError(
                              err,
                              `sections[${i}].fields[${j}].label`,
                            )}
                          >
                            <Input
                              value={f.label}
                              onChange={(e) =>
                                updateField(i, j, { label: e.target.value })
                              }
                            />
                          </FormField>
                          <FormField
                            label="Type"
                            error={fieldError(
                              err,
                              `sections[${i}].fields[${j}].type`,
                            )}
                          >
                            <Select
                              value={f.type}
                              onChange={(e) =>
                                updateField(i, j, { type: e.target.value })
                              }
                            >
                              {palette.map((p) => (
                                <option key={p}>{p}</option>
                              ))}
                            </Select>
                          </FormField>
                          <FormField label="Help">
                            <Input
                              value={f.help}
                              onChange={(e) =>
                                updateField(i, j, { help: e.target.value })
                              }
                            />
                          </FormField>
                          {["select", "multiselect"].includes(f.type) && (
                            <FormField
                              label="Options, one per line"
                              error={fieldError(
                                err,
                                `sections[${i}].fields[${j}].options`,
                              )}
                            >
                              <Textarea
                                value={(f.options ?? []).join("\n")}
                                onChange={(e) =>
                                  updateField(i, j, {
                                    options: e.target.value
                                      .split("\n")
                                      .filter(Boolean),
                                  })
                                }
                              />
                            </FormField>
                          )}
                          <label className="flex items-center gap-2">
                            <Checkbox
                              checked={f.required}
                              onChange={(e) =>
                                updateField(i, j, {
                                  required: e.target.checked,
                                })
                              }
                            />
                            Required
                          </label>
                        </div>
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() =>
                            updateSection(i, {
                              fields: section.fields.filter((_, x) => x !== j),
                            })
                          }
                        >
                          Remove field
                        </Button>
                      </div>
                    ))}
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() =>
                        updateSection(i, {
                          fields: [...section.fields, makeField()],
                        })
                      }
                    >
                      Add field
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setSchema((s) => ({
                          ...s,
                          sections: s.sections.filter((_, x) => x !== i),
                        }))
                      }
                    >
                      Remove section
                    </Button>
                  </section>
                ))}
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    setSchema((s) => ({
                      ...s,
                      sections: [...s.sections, makeSection()],
                    }))
                  }
                >
                  Add section
                </Button>
                <h3 className="font-semibold">Required documents</h3>
                {schema.required_documents.map((d, i) => (
                  <div
                    key={i}
                    className="grid gap-2 rounded border p-3 sm:grid-cols-4"
                  >
                    {(["key", "label", "help"] as const).map((k) => (
                      <FormField
                        key={k}
                        label={k}
                        error={fieldError(err, `required_documents[${i}].${k}`)}
                      >
                        <Input
                          value={d[k]}
                          onChange={(e) =>
                            setSchema((s) => ({
                              ...s,
                              required_documents: s.required_documents.map(
                                (x, j) =>
                                  j === i ? { ...x, [k]: e.target.value } : x,
                              ),
                            }))
                          }
                        />
                      </FormField>
                    ))}
                    <FormField label="Category">
                      <Select
                        value={d.category}
                        onChange={(e) =>
                          setSchema((s) => ({
                            ...s,
                            required_documents: s.required_documents.map(
                              (x, j) =>
                                j === i
                                  ? { ...x, category: e.target.value }
                                  : x,
                            ),
                          }))
                        }
                      >
                        {[
                          "application",
                          "personal",
                          "decision",
                          "result",
                          "other",
                        ].map((c) => (
                          <option key={c}>{c}</option>
                        ))}
                      </Select>
                    </FormField>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setSchema((s) => ({
                          ...s,
                          required_documents: s.required_documents.filter(
                            (_, j) => j !== i,
                          ),
                        }))
                      }
                    >
                      Remove
                    </Button>
                  </div>
                ))}
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    setSchema((s) => ({
                      ...s,
                      required_documents: [
                        ...s.required_documents,
                        makeDocument(),
                      ],
                    }))
                  }
                >
                  Add required document
                </Button>
                {err && <Banner tone="error">{err.message}</Banner>}
                <Button
                  onClick={() => void persist()}
                  disabled={save.isPending}
                >
                  {save.isPending ? "Saving…" : "Save draft"}
                </Button>
              </div>
            ) : (
              <p className="text-sm text-slate-600">
                This version is read-only.
              </p>
            )}
          </CardBody>
        </Card>
      )}
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Publish template version"
        footer={
          <>
            <Button variant="secondary" onClick={() => setConfirm(false)}>
              Cancel
            </Button>
            <Button
              onClick={() => void doPublish()}
              disabled={publish.isPending}
            >
              {publish.isPending ? "Publishing…" : "Publish"}
            </Button>
          </>
        }
      >
        <p>
          Publishing applies to new submissions only; submitted applications
          keep their version.
        </p>
      </Dialog>
    </>
  );
}
