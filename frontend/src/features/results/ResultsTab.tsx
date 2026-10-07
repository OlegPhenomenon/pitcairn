import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { apiGet, fieldError, useApiQuery } from "../../api/client";
import type {
  CreateDeliverableRequest,
  DeliverableDto,
  ListResponse,
  UpdateDeliverableRequest,
  CoordinatorDto,
} from "../../api/types";
import { formatDate, toNum, titleize } from "../../lib/format";
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
  Select,
  Skeleton,
  StatusBadge,
  Table,
  Textarea,
  useToast,
} from "../../ui";
import { FileUpload } from "../upload/FileUpload";
import {
  projectDocsKey,
  useCreateDocument,
  useProjectDocuments,
} from "../projects/api";
import { useProject, projectKey } from "../projects/api";
import { useProjectContext } from "../projects/ProjectLayout";
import {
  useCloseProject,
  useCreateDeliverable,
  usePublish,
  useResultAction,
  useSubmissionHistory,
  useSubmitResult,
  useUpdateDeliverable,
} from "./api";
import { agreementText, isOverdue } from "./display";

const kinds = ["report", "dataset", "media", "samples", "other"];

export function ResultsTab() {
  const { project, me } = useProjectContext();
  const workspace = useProject(project.id);
  const coordinator = Boolean(me?.user.roles.includes("coordinator"));
  const editor = ["team_editor", "team_lead"].includes(project.my_access);
  const [formOpen, setFormOpen] = useState(false);
  const [selected, setSelected] = useState<DeliverableDto | null>(null);
  const [closeOpen, setCloseOpen] = useState(false);
  const items = workspace.data?.results.deliverables ?? [];
  return (
    <div className="space-y-5">
      <Card>
        <CardHeader
          title={
            coordinator
              ? "What we expect from the team"
              : "What I must deliver to Pitcairn"
          }
          actions={
            <div className="flex gap-2">
              {(coordinator || editor) && (
                <Button
                  size="sm"
                  onClick={() => {
                    setSelected(null);
                    setFormOpen(true);
                  }}
                >
                  Propose deliverable
                </Button>
              )}
              {coordinator && project.status === "approved" && (
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setCloseOpen(true)}
                >
                  Close project
                </Button>
              )}
            </div>
          }
        />
        <CardBody>
          {workspace.isError && (
            <Banner tone="error">{workspace.error.message}</Banner>
          )}
          {workspace.isPending ? (
            <div className="space-y-2">
              <Skeleton className="h-12" />
              <Skeleton className="h-12" />
              <Skeleton className="h-12" />
            </div>
          ) : (
            <Table
              rows={items}
              rowKey={(d) => d.id}
              empty={{
                title: "No deliverables yet",
                body: "Propose what the research team will send to Pitcairn.",
              }}
              columns={[
                {
                  header: "Deliverable",
                  cell: (d) => (
                    <button
                      className="text-left font-semibold text-teal-800 underline"
                      onClick={() => setSelected(d)}
                    >
                      {d.title}
                    </button>
                  ),
                },
                { header: "Kind", cell: (d) => titleize(d.kind) },
                {
                  header: "Due",
                  cell: (d) => (
                    <time
                      dateTime={d.due_date}
                      title={d.due_date}
                      className={
                        isOverdue(d) ? "font-semibold text-red-700" : ""
                      }
                    >
                      {formatDate(d.due_date)} {isOverdue(d) && "· Overdue"}
                    </time>
                  ),
                },
                {
                  header: "From → to",
                  cell: (d) => (
                    <>
                      {d.sender_name} → {d.recipient_name}
                    </>
                  ),
                  hideOnCard: true,
                },
                { header: "Agreement", cell: (d) => agreementText(d) },
                {
                  header: "Status",
                  cell: (d) => (
                    <StatusBadge
                      status={d.status}
                      label={
                        d.status === "accepted"
                          ? "Results receipt accepted"
                          : undefined
                      }
                    />
                  ),
                },
              ]}
            />
          )}
        </CardBody>
      </Card>
      {selected && (
        <DeliverableDetail
          key={selected.id}
          item={items.find((d) => d.id === selected.id) ?? selected}
          editor={editor}
          coordinator={coordinator}
          demo={Boolean(me?.demo_mode)}
          onEdit={() => setFormOpen(true)}
          onClose={() => setSelected(null)}
        />
      )}
      <DeliverableForm
        key={`form-${selected?.id ?? "new"}`}
        open={formOpen}
        onClose={() => setFormOpen(false)}
        projectId={project.id}
        item={selected}
        team={
          workspace.data?.application.team.filter((m) => !m.removed_at) ?? []
        }
        meId={me?.user.id ?? ""}
        coordinator={coordinator}
      />
      {coordinator && (
        <CloseDialog
          open={closeOpen}
          onClose={() => setCloseOpen(false)}
          projectId={project.id}
        />
      )}
    </div>
  );
}

function DeliverableForm({
  open,
  onClose,
  projectId,
  item,
  team,
  meId,
  coordinator,
}: {
  open: boolean;
  onClose: () => void;
  projectId: string;
  item: DeliverableDto | null;
  team: { user_id: string; name: string }[];
  meId: string;
  coordinator: boolean;
}) {
  const create = useCreateDeliverable(projectId),
    update = useUpdateDeliverable(projectId, item?.id ?? "");
  const coordinators = useApiQuery<ListResponse<CoordinatorDto>>(
    ["coordinators"],
    "/coordinators",
    { enabled: open },
  );
  const toast = useToast();
  const [title, setTitle] = useState(item?.title ?? "");
  const [description, setDescription] = useState(item?.description ?? "");
  const [kind, setKind] = useState(item?.kind ?? "report");
  const [due, setDue] = useState(item?.due_date ?? "");
  const [sender, setSender] = useState(
    item?.sender_id ??
      team.find((m) => m.user_id === meId)?.user_id ??
      team[0]?.user_id ??
      "",
  );
  const [recipient, setRecipient] = useState(
    item?.recipient_id ?? (coordinator ? meId : ""),
  );
  const [reason, setReason] = useState("");
  const error = create.error ?? update.error;
  const busy = create.isPending || update.isPending;
  async function save(e: FormEvent) {
    e.preventDefault();
    try {
      if (item) {
        const body: UpdateDeliverableRequest = {
          title,
          description,
          kind,
          due_date: due,
          sender_id: sender,
          recipient_id: recipient,
          reason: due !== item.due_date ? reason : null,
        };
        await update.mutateAsync(body);
      } else {
        const body: CreateDeliverableRequest = {
          title,
          description,
          kind,
          due_date: due,
          sender_id: sender,
          recipient_id: recipient,
        };
        await create.mutateAsync(body);
      }
      toast.success(item ? "Deliverable updated" : "Deliverable proposed");
      onClose();
    } catch {
      /* mutation exposes errors */
    }
  }
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={item ? "Edit deliverable" : "Propose deliverable"}
      footer={
        <>
          <Button type="button" variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="deliverable-form" disabled={busy}>
            {busy ? "Saving…" : "Save proposal"}
          </Button>
        </>
      }
    >
      <form
        id="deliverable-form"
        onSubmit={(e) => void save(e)}
        className="space-y-3"
      >
        <p className="text-sm text-slate-600">
          A change to agreed terms asks the other side to agree again.
        </p>
        <FormField label="Title" required error={fieldError(error, "title")}>
          <Input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            required
          />
        </FormField>
        <FormField label="Description" error={fieldError(error, "description")}>
          <Textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </FormField>
        <FormField label="Kind" error={fieldError(error, "kind")}>
          <Select value={kind} onChange={(e) => setKind(e.target.value)}>
            {kinds.map((k) => (
              <option key={k} value={k}>
                {titleize(k)}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField
          label="Due date"
          required
          error={fieldError(error, "due_date")}
        >
          <Input
            type="date"
            value={due}
            onChange={(e) => setDue(e.target.value)}
            required
          />
        </FormField>
        {item && due !== item.due_date && (
          <FormField
            label="Reason for due date change"
            required
            error={fieldError(error, "reason")}
          >
            <Textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              required
            />
          </FormField>
        )}
        <FormField
          label="Sender"
          required
          error={fieldError(error, "sender_id")}
        >
          <Select
            value={sender}
            onChange={(e) => setSender(e.target.value)}
            required
          >
            <option value="">Choose a team member</option>
            {team.map((m) => (
              <option key={m.user_id} value={m.user_id}>
                {m.name}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField
          label="Recipient"
          required
          error={fieldError(error, "recipient_id")}
        >
          <Select
            value={recipient}
            onChange={(e) => setRecipient(e.target.value)}
            required
          >
            <option value="">Choose a coordinator</option>
            {coordinators.data?.items.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </Select>
        </FormField>
        {error && <Banner tone="error">{error.message}</Banner>}
      </form>
    </Dialog>
  );
}

function DeliverableDetail({
  item,
  editor,
  coordinator,
  demo,
  onEdit,
  onClose,
}: {
  item: DeliverableDto;
  editor: boolean;
  coordinator: boolean;
  demo: boolean;
  onEdit: () => void;
  onClose: () => void;
}) {
  const action = useResultAction(item.project_id),
    history = useSubmissionHistory(item.project_id, item.id),
    toast = useToast();
  const [note, setNote] = useState("");
  const [review, setReview] = useState<"changes" | "accept" | "waive" | null>(
    null,
  );
  const canAgree =
    item.status === "proposed" &&
    ((editor && !item.team_agreed_at) ||
      (coordinator && !item.staff_agreed_at));
  async function act(path: string, body?: unknown) {
    try {
      await action.mutateAsync({ path, body });
      toast.success("Deliverable updated");
      setReview(null);
    } catch {
      /* toast from hook */
    }
  }
  return (
    <Card>
      <CardHeader
        title={item.title}
        actions={
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close details
          </Button>
        }
      />
      <CardBody className="space-y-4">
        <p className="text-sm text-slate-700">
          {item.description || "No description provided."}
        </p>
        <div className="flex flex-wrap gap-2 text-sm">
          <StatusBadge
            status={item.status}
            label={
              item.status === "accepted"
                ? "Results receipt accepted"
                : undefined
            }
          />
          <span>{agreementText(item)}</span>
          <span>Terms version {toNum(item.terms_version)}</span>
        </div>
        <p className="text-sm">
          Due{" "}
          <time dateTime={item.due_date} title={item.due_date}>
            {formatDate(item.due_date)}
          </time>{" "}
          · {item.sender_name} sends to {item.recipient_name}
        </p>
        {(editor || coordinator) &&
          ["proposed", "agreed", "submitted", "changes_requested"].includes(
            item.status,
          ) && (
            <Button size="sm" variant="secondary" onClick={onEdit}>
              Edit terms
            </Button>
          )}
        {canAgree && (
          <Button
            size="sm"
            disabled={action.isPending}
            onClick={() => void act(`/deliverables/${item.id}/agree`)}
          >
            {action.isPending ? "Agreeing…" : "Agree to these terms"}
          </Button>
        )}
        {editor && ["agreed", "changes_requested"].includes(item.status) && (
          <SubmitResult item={item} demo={demo} />
        )}
        <section>
          <h3 className="font-semibold">Submission history</h3>
          {history.isPending ? (
            <Skeleton className="mt-2 h-20" />
          ) : history.isError ? (
            <Banner tone="error">{history.error.message}</Banner>
          ) : !history.data?.items.length ? (
            <EmptyState title="No results submitted yet" />
          ) : (
            <div className="mt-2 space-y-2">
              {history.data.items.map((s) => (
                <div
                  key={s.id}
                  className="rounded border border-slate-200 p-3 text-sm"
                >
                  <div className="flex gap-2">
                    <strong>Submission {toNum(s.number)}</strong>
                    <StatusBadge
                      status={s.status}
                      label={
                        s.status === "accepted" ? "Receipt accepted" : undefined
                      }
                    />
                  </div>
                  <p>
                    <time dateTime={s.submitted_at} title={s.submitted_at}>
                      {formatDate(s.submitted_at)}
                    </time>{" "}
                    · {s.note || "No note"}
                  </p>
                  {s.review_note && (
                    <p className="mt-1 text-amber-900">
                      Review note: {s.review_note}
                    </p>
                  )}
                  {s.files.map((f) => (
                    <p key={f.document_version_id}>{f.title}</p>
                  ))}
                  {s.links.map((l) => (
                    <p key={l.id}>
                      <a
                        className="text-teal-700 underline"
                        href={l.url}
                        target="_blank"
                        rel="noreferrer"
                      >
                        {l.description}
                      </a>{" "}
                      · {l.version_label} ·{" "}
                      <StatusBadge
                        status={
                          l.available === false
                            ? "failed"
                            : l.available === true
                              ? "active"
                              : "pending"
                        }
                        label={
                          l.available === false
                            ? "Unavailable"
                            : l.available === true
                              ? "Available"
                              : "Checking"
                        }
                      />
                    </p>
                  ))}
                </div>
              ))}
            </div>
          )}
        </section>
        {coordinator && item.latest_submission?.status === "received" && (
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setReview("changes")}
            >
              Request changes
            </Button>
            <Button size="sm" onClick={() => setReview("accept")}>
              Accept receipt
            </Button>
          </div>
        )}
        {coordinator &&
          ["proposed", "agreed", "submitted", "changes_requested"].includes(
            item.status,
          ) && (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setReview("waive")}
            >
              Waive deliverable
            </Button>
          )}
        {coordinator && item.status === "accepted" && (
          <PublicationPanel item={item} />
        )}
        <Dialog
          open={review !== null}
          onClose={() => setReview(null)}
          title={
            review === "accept"
              ? "Accept receipt"
              : review === "waive"
                ? "Waive deliverable"
                : "Request changes"
          }
          footer={
            <>
              <Button variant="secondary" onClick={() => setReview(null)}>
                Cancel
              </Button>
              <Button
                disabled={
                  action.isPending || (review !== "accept" && !note.trim())
                }
                onClick={() =>
                  void act(
                    review === "accept"
                      ? `/submissions/${item.latest_submission?.id}/accept`
                      : review === "waive"
                        ? `/deliverables/${item.id}/waive`
                        : `/submissions/${item.latest_submission?.id}/request-changes`,
                    review === "accept" ? undefined : { note },
                  )
                }
              >
                {action.isPending ? "Saving…" : "Confirm"}
              </Button>
            </>
          }
        >
          <p className="mb-3 text-sm text-slate-700">
            {review === "accept"
              ? "This confirms Pitcairn received the agreed material. It does not validate the scientific findings."
              : review === "waive"
                ? "This ends the obligation to deliver this item. The note will be recorded."
                : "The team will be asked to correct this submission."}
          </p>
          {review !== "accept" && (
            <FormField
              label="Note"
              required
              error={fieldError(action.error, "note")}
            >
              <Textarea
                value={note}
                onChange={(e) => setNote(e.target.value)}
              />
            </FormField>
          )}
        </Dialog>
      </CardBody>
    </Card>
  );
}

function SubmitResult({ item, demo }: { item: DeliverableDto; demo: boolean }) {
  const docs = useProjectDocuments(item.project_id),
    create = useCreateDocument(item.project_id),
    submit = useSubmitResult(item.project_id, item.id),
    query = useQueryClient(),
    toast = useToast();
  const [title, setTitle] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [dictionary, setDictionary] = useState([
    { column: "", description: "", unit: "", method: "" },
  ]);
  const [links, setLinks] = useState([
    { url: "", description: "", version_label: "", access_notes: "" },
  ]);
  const resultDocs = (docs.data?.items ?? []).filter(
    (d) => d.category === "result" && d.latest_version,
  );
  const onFile = async (fileId: string) => {
    try {
      await create.mutateAsync({
        title,
        category: "result",
        slot_key: null,
        file_id: fileId,
        note: null,
      });
      setTitle("");
      await query.invalidateQueries({
        queryKey: projectDocsKey(item.project_id),
      });
      toast.success(
        "Result file uploaded",
        "Select it after the scan finishes before submitting.",
      );
    } catch {
      /* hook handles error */
    }
  };
  async function send(e: FormEvent) {
    e.preventDefault();
    try {
      await submit.mutateAsync({
        note: note || null,
        document_version_ids: chosen,
        data_dictionary:
          item.kind === "dataset" ? dictionary.filter((d) => d.column) : null,
        links: links
          .filter((l) => l.url)
          .map((l) => ({ ...l, access_notes: l.access_notes || null })),
      });
      toast.success("Results submitted");
      setChosen([]);
    } catch {
      /* inline errors */
    }
  }
  return (
    <section className="rounded-lg border border-teal-200 bg-teal-50/50 p-4">
      <h3 className="font-semibold">Submit results</h3>
      <form onSubmit={(e) => void send(e)} className="mt-3 space-y-3">
        <FormField label="File title" error={fieldError(create.error, "title")}>
          <Input value={title} onChange={(e) => setTitle(e.target.value)} />
        </FormField>
        <FileUpload
          demoMode={demo}
          disabled={!title.trim() || create.isPending}
          onComplete={(fileId) => void onFile(fileId)}
          hint="Files upload in resumable chunks."
        />
        {docs.isPending && <Skeleton className="h-10" />}
        {docs.isError && <Banner tone="error">{docs.error.message}</Banner>}
        {resultDocs.length > 0 && (
          <fieldset>
            <legend className="text-sm font-medium">Files to include</legend>
            {resultDocs.map((d) => (
              <label key={d.id} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={chosen.includes(d.latest_version!.id)}
                  disabled={d.latest_version?.scan_status !== "clean"}
                  onChange={(e) =>
                    setChosen((v) =>
                      e.target.checked
                        ? [...v, d.latest_version!.id]
                        : v.filter((x) => x !== d.latest_version!.id),
                    )
                  }
                />
                {d.title} ·{" "}
                <StatusBadge status={d.latest_version!.scan_status} />
              </label>
            ))}
          </fieldset>
        )}
        {item.kind === "dataset" && (
          <section>
            <h4 className="font-medium">Data dictionary</h4>
            <a
              className="text-sm text-teal-700 underline"
              href="/measurement-format.csv"
              download
            >
              Measurement CSV format: site,date,variable,value,unit
            </a>
            {dictionary.map((d, i) => (
              <div key={i} className="mt-2 grid gap-2 sm:grid-cols-2">
                {(["column", "description", "unit", "method"] as const).map(
                  (k) => (
                    <FormField
                      key={k}
                      label={titleize(k)}
                      error={fieldError(
                        submit.error,
                        `data_dictionary[${i}].${k}`,
                      )}
                    >
                      <Input
                        value={d[k]}
                        onChange={(e) =>
                          setDictionary((v) =>
                            v.map((row, j) =>
                              j === i ? { ...row, [k]: e.target.value } : row,
                            ),
                          )
                        }
                      />
                    </FormField>
                  ),
                )}
              </div>
            ))}
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() =>
                setDictionary((v) => [
                  ...v,
                  { column: "", description: "", unit: "", method: "" },
                ])
              }
            >
              Add column
            </Button>
          </section>
        )}
        <section>
          <h4 className="font-medium">External links</h4>
          {links.map((l, i) => (
            <div key={i} className="mt-2 grid gap-2 sm:grid-cols-2">
              {(
                ["url", "description", "version_label", "access_notes"] as const
              ).map((k) => (
                <FormField
                  key={k}
                  label={titleize(k)}
                  error={fieldError(submit.error, `links[${i}].${k}`)}
                >
                  <Input
                    type={k === "url" ? "url" : "text"}
                    value={l[k]}
                    onChange={(e) =>
                      setLinks((v) =>
                        v.map((row, j) =>
                          j === i ? { ...row, [k]: e.target.value } : row,
                        ),
                      )
                    }
                  />
                </FormField>
              ))}
            </div>
          ))}
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() =>
              setLinks((v) => [
                ...v,
                {
                  url: "",
                  description: "",
                  version_label: "",
                  access_notes: "",
                },
              ])
            }
          >
            Add link
          </Button>
        </section>
        <FormField
          label="Submission note"
          error={fieldError(submit.error, "note")}
        >
          <Textarea value={note} onChange={(e) => setNote(e.target.value)} />
        </FormField>
        {submit.error && <Banner tone="error">{submit.error.message}</Banner>}
        <Button type="submit" disabled={submit.isPending}>
          {submit.isPending ? "Submitting…" : "Submit results"}
        </Button>
      </form>
    </section>
  );
}

function PublicationPanel({ item }: { item: DeliverableDto }) {
  return (
    <div className="rounded border border-slate-200 p-3">
      <h3 className="font-semibold">Publication</h3>
      <p className="text-sm">
        Current level: {titleize(item.publish_level)}.{" "}
        {item.embargo_until && (
          <>
            Files available from{" "}
            <time title={item.embargo_until}>
              {formatDate(item.embargo_until)}
            </time>
            .
          </>
        )}
      </p>
      <PublicationDialog item={item} />
    </div>
  );
}

function PublicationDialog({ item }: { item: DeliverableDto }) {
  const [open, setOpen] = useState(false);
  const [level, setLevel] = useState(item.publish_level);
  const [embargo, setEmbargo] = useState(item.embargo_until ?? "");
  const [files, setFiles] = useState<string[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const publish = usePublish(item.project_id, item.id);
  const toast = useToast();
  const accepted = item.accepted_submission;
  async function save() {
    try {
      await publish.mutateAsync({
        publication: { publish_level: level, embargo_until: embargo || null },
        files: { document_version_ids: files },
      });
      toast.success("Publication updated");
      setOpen(false);
    } catch {
      /* hook handles error */
    }
  }
  return (
    <>
      <Button
        size="sm"
        disabled={loadingFiles}
        onClick={async () => {
          setLoadingFiles(true);
          try {
            const selection = await apiGet<{ document_version_ids: string[] }>(
              `/deliverables/${item.id}/publication-files`,
            );
            setFiles(selection.document_version_ids);
            setLevel(item.publish_level);
            setEmbargo(item.embargo_until ?? "");
            setOpen(true);
          } catch (error) {
            toast.error(
              "Could not load publication settings",
              error instanceof Error ? error.message : undefined,
            );
          } finally {
            setLoadingFiles(false);
          }
        }}
      >
        {loadingFiles ? "Loading…" : "Set publication"}
      </Button>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title="Publish results"
        footer={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => void save()} disabled={publish.isPending}>
              {publish.isPending ? "Publishing…" : "Publish"}
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <Banner tone="warning">
            Check every public file for sensitive coordinates and personal data
            before publishing.
          </Banner>
          <FormField
            label="Publication level"
            error={fieldError(publish.error, "publish_level")}
          >
            <Select value={level} onChange={(e) => setLevel(e.target.value)}>
              <option value="none">None</option>
              <option value="metadata">Metadata only</option>
              <option value="metadata_and_files">
                Metadata and selected files
              </option>
            </Select>
          </FormField>
          <FormField
            label="Files available from"
            error={fieldError(publish.error, "embargo_until")}
          >
            <Input
              type="date"
              value={embargo}
              onChange={(e) => setEmbargo(e.target.value)}
            />
          </FormField>
          {level === "metadata_and_files" && (
            <fieldset>
              <legend className="font-medium">Public files</legend>
              {accepted?.files.map((f) => (
                <label key={f.document_version_id} className="flex gap-2">
                  <Checkbox
                    checked={files.includes(f.document_version_id)}
                    onChange={(e) =>
                      setFiles((v) =>
                        e.target.checked
                          ? [...v, f.document_version_id]
                          : v.filter((x) => x !== f.document_version_id),
                      )
                    }
                  />
                  {f.title}
                </label>
              ))}
            </fieldset>
          )}
          <div className="rounded bg-sand-50 p-3 text-sm">
            <strong>Public preview</strong>
            <p>
              {item.title} · {item.kind}
            </p>
            <p>{item.description}</p>
            {level === "metadata_and_files" && (
              <p>
                {embargo
                  ? `Files available from ${formatDate(embargo)}`
                  : `${files.length} selected file(s) will be downloadable`}
              </p>
            )}
          </div>
          {publish.error && (
            <Banner tone="error">{publish.error.message}</Banner>
          )}
        </div>
      </Dialog>
    </>
  );
}

export function CloseDialog({
  open,
  onClose,
  projectId,
}: {
  open: boolean;
  onClose: () => void;
  projectId: string;
}) {
  const close = useCloseProject(projectId),
    project = useProject(projectId),
    toast = useToast(),
    query = useQueryClient();
  const [resolutions, setResolutions] = useState<
    Record<string, { action: string; note: string }>
  >({});
  const unresolved = (project.data?.results.deliverables ?? []).filter((d) =>
    ["proposed", "agreed", "submitted", "changes_requested"].includes(d.status),
  );
  async function submit() {
    try {
      await close.mutateAsync({
        deliverable_resolutions: unresolved.map((d) => ({
          deliverable_id: d.id,
          action: resolutions[d.id]?.action ?? "waive",
          note: resolutions[d.id]?.note ?? "",
        })),
      });
      toast.success("Project closed");
      onClose();
    } catch (e) {
      if (
        e instanceof Error &&
        "code" in e &&
        e.code === "unresolved_deliverables"
      )
        await query.invalidateQueries({ queryKey: projectKey(projectId) });
    }
  }
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Close project"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => void submit()}
            disabled={
              close.isPending ||
              unresolved.some((d) => !resolutions[d.id]?.note.trim())
            }
          >
            {close.isPending ? "Closing…" : "Close project"}
          </Button>
        </>
      }
    >
      <p className="text-sm text-slate-700">
        Closing the project ends work on its open deliverables. Each unresolved
        item needs a recorded waiver or cancellation.
      </p>
      {unresolved.map((d) => (
        <div key={d.id} className="mt-3 rounded border p-3">
          <strong>{d.title}</strong>
          <p className="text-sm">
            <StatusBadge status={d.status} />
          </p>
          <FormField label="Resolution">
            <Select
              value={resolutions[d.id]?.action ?? "waive"}
              onChange={(e) =>
                setResolutions((v) => ({
                  ...v,
                  [d.id]: { action: e.target.value, note: v[d.id]?.note ?? "" },
                }))
              }
            >
              <option value="waive">Waive</option>
              <option value="cancel">Cancel</option>
            </Select>
          </FormField>
          <FormField
            label="Required note"
            error={fieldError(
              close.error,
              `deliverable_resolutions[${unresolved.indexOf(d)}].note`,
            )}
          >
            <Textarea
              value={resolutions[d.id]?.note ?? ""}
              onChange={(e) =>
                setResolutions((v) => ({
                  ...v,
                  [d.id]: {
                    action: v[d.id]?.action ?? "waive",
                    note: e.target.value,
                  },
                }))
              }
            />
          </FormField>
        </div>
      ))}
      {close.error && <Banner tone="error">{close.error.message}</Banner>}
    </Dialog>
  );
}
