import { useState } from "react";
import type { ResourceDto } from "../../api/generated/ResourceDto";
import { fieldError, useApiQuery } from "../../api/client";
import type { ListResponse, UserDto } from "../../api/types";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Dialog,
  EmptyState,
  FormField,
  Input,
  PageHeader,
  PageLoading,
  Select,
  StatusBadge,
  Table,
  Textarea,
  useToast,
} from "../../ui";
import { useMe } from "../auth/api";
import { DateText, money } from "./display";
import {
  useCreateResource,
  usePatchResource,
  useCreateTariff,
  useResources,
  useTariffs,
} from "./api";
const kinds = ["room", "lab", "equipment", "boat", "service"];
export function ResourcesPage() {
  const me = useMe();
  const allowed = !!me.data?.user.roles.includes("admin");
  const resources = useResources(allowed);
  const users = useApiQuery<ListResponse<UserDto>>(
    ["resources", "providers"],
    "/admin/users?limit=200",
    { enabled: allowed },
  );
  const [edit, setEdit] = useState<ResourceDto | "new" | null>(null);
  const [tariffResource, setTariffResource] = useState<ResourceDto | null>(
    null,
  );
  const [tariffOpen, setTariffOpen] = useState(false);
  const [kind, setKind] = useState("room"),
    [name, setName] = useState(""),
    [description, setDescription] = useState(""),
    [quantity, setQuantity] = useState("1"),
    [unitLabel, setUnitLabel] = useState("unit"),
    [provider, setProvider] = useState(""),
    [active, setActive] = useState(true);
  const [unit, setUnit] = useState("per_night"),
    [price, setPrice] = useState(""),
    [from, setFrom] = useState("");
  const toast = useToast();
  const create = useCreateResource(),
    patch = usePatchResource(),
    createTariff = useCreateTariff(tariffResource?.id ?? "");
  const tariffs = useTariffs(tariffResource?.id ?? "");
  const resourceError = create.error ?? patch.error;
  const open = (r: ResourceDto | "new") => {
    setEdit(r);
    setKind(r === "new" ? "room" : r.kind);
    setName(r === "new" ? "" : r.name);
    setDescription(r === "new" ? "" : r.description);
    setQuantity(r === "new" ? "1" : String(r.quantity));
    setUnitLabel(r === "new" ? "unit" : r.unit_label);
    setProvider(r === "new" ? "" : (r.provider_user_id ?? ""));
    setActive(r === "new" || r.active);
    create.reset();
    patch.reset();
  };
  if (!allowed)
    return (
      <Banner tone="error">
        Resource administration requires an admin role.
      </Banner>
    );
  return (
    <div className="space-y-5">
      <PageHeader
        title="Resources and tariffs"
        subtitle="Manage bookable rooms, equipment and services"
        actions={<Button onClick={() => open("new")}>Add resource</Button>}
      />
      {resources.isPending ? (
        <PageLoading />
      ) : resources.isError ? (
        <Banner tone="error">
          {resources.error.message}{" "}
          <button
            className="underline"
            onClick={() => void resources.refetch()}
          >
            Try again
          </button>
        </Banner>
      ) : (
        <Table
          rows={resources.data?.items ?? []}
          rowKey={(r) => r.id}
          empty={{ title: "No resources yet" }}
          columns={[
            {
              header: "Resource",
              cell: (r) => (
                <>
                  <strong>{r.name}</strong>
                  <span className="block text-slate-600">{r.description}</span>
                </>
              ),
            },
            { header: "Kind", cell: (r) => r.kind },
            {
              header: "Capacity",
              cell: (r) => `${toNum(r.quantity)} ${r.unit_label}`,
            },
            {
              header: "Provider",
              cell: (r) => r.provider_name ?? "Base managed",
            },
            {
              header: "Status",
              cell: (r) => (
                <StatusBadge status={r.active ? "active" : "disabled"} />
              ),
            },
            {
              header: "Actions",
              cell: (r) => (
                <span className="flex flex-wrap gap-1">
                  <Button size="sm" variant="secondary" onClick={() => open(r)}>
                    Edit
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setTariffResource(r)}
                  >
                    Tariffs
                  </Button>
                </span>
              ),
            },
          ]}
        />
      )}
      <Dialog
        open={!!edit}
        onClose={() => setEdit(null)}
        title={edit === "new" ? "Add resource" : "Edit resource"}
        footer={
          <>
            <Button variant="secondary" onClick={() => setEdit(null)}>
              Close
            </Button>
            <Button
              form="resource-form"
              type="submit"
              loading={create.isPending || patch.isPending}
            >
              Save resource
            </Button>
          </>
        }
      >
        <form
          id="resource-form"
          className="space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!edit) return;
            try {
              if (edit === "new")
                await create.mutateAsync({
                  kind,
                  name,
                  description,
                  quantity: BigInt(quantity),
                  unit_label: unitLabel,
                  provider_user_id: provider || null,
                  active,
                });
              else
                await patch.mutateAsync({
                  id: edit.id,
                  body: {
                    name,
                    description,
                    quantity: BigInt(quantity),
                    unit_label: unitLabel,
                    provider_user_id: provider || null,
                    clear_provider: !provider && !!edit.provider_user_id,
                    active,
                  },
                });
              toast.success("Resource saved");
              setEdit(null);
            } catch {
              /* inline */
            }
          }}
        >
          <FormField
            label="Kind"
            required
            error={fieldError(resourceError, "kind")}
          >
            <Select
              value={kind}
              onChange={(e) => setKind(e.target.value)}
              disabled={edit !== "new"}
            >
              {kinds.map((k) => (
                <option key={k}>{k}</option>
              ))}
            </Select>
          </FormField>
          <FormField
            label="Name"
            required
            error={fieldError(resourceError, "name")}
          >
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="Description"
            error={fieldError(resourceError, "description")}
          >
            <Textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </FormField>
          <FormField
            label="Quantity"
            required
            error={fieldError(resourceError, "quantity")}
          >
            <Input
              type="number"
              min="1"
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="Unit label"
            error={fieldError(resourceError, "unit_label")}
          >
            <Input
              value={unitLabel}
              onChange={(e) => setUnitLabel(e.target.value)}
            />
          </FormField>
          <FormField
            label="Provider user"
            error={fieldError(resourceError, "provider_user_id")}
          >
            <Select
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
            >
              <option value="">Base managed</option>
              {users.data?.items
                .filter((u) => u.roles.includes("provider"))
                .map((u) => (
                  <option key={u.id} value={u.id}>
                    {u.name}
                  </option>
                ))}
            </Select>
          </FormField>
          {users.isError && (
            <Banner tone="error">Could not load providers.</Banner>
          )}
          <label className="flex gap-2 text-sm">
            <input
              type="checkbox"
              checked={active}
              onChange={(e) => setActive(e.target.checked)}
            />
            Active
          </label>
        </form>
      </Dialog>
      <Dialog
        open={!!tariffResource && !tariffOpen}
        onClose={() => setTariffResource(null)}
        title={`${tariffResource?.name ?? ""} tariffs`}
        footer={
          <Button variant="secondary" onClick={() => setTariffResource(null)}>
            Close
          </Button>
        }
      >
        <p className="mb-3 text-sm text-slate-600">
          New tariffs apply from their effective date. Existing invoices keep
          their prices.
        </p>
        {tariffs.isPending ? (
          <PageLoading />
        ) : tariffs.isError ? (
          <Banner tone="error">{tariffs.error.message}</Banner>
        ) : tariffs.data?.items.length ? (
          <Table
            rows={tariffs.data.items}
            rowKey={(t) => t.id}
            columns={[
              {
                header: "From",
                cell: (t) => <DateText value={t.effective_from} />,
              },
              { header: "Unit", cell: (t) => t.unit.replace("_", " ") },
              {
                header: "Price",
                cell: (t) => money(t.price_cents, t.currency),
              },
            ]}
          />
        ) : (
          <EmptyState title="No tariffs yet" />
        )}
        <Button
          className="mt-3"
          onClick={() => {
            setTariffOpen(true);
            setPrice("");
            setFrom("");
          }}
        >
          New tariff from date
        </Button>
      </Dialog>
      <Dialog
        open={tariffOpen}
        onClose={() => setTariffOpen(false)}
        title="New tariff from date"
        footer={
          <>
            <Button variant="secondary" onClick={() => setTariffOpen(false)}>
              Close
            </Button>
            <Button
              form="tariff-form"
              type="submit"
              loading={createTariff.isPending}
            >
              Save tariff
            </Button>
          </>
        }
      >
        <form
          id="tariff-form"
          className="space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            try {
              await createTariff.mutateAsync({
                unit,
                price_cents: BigInt(Math.round(Number(price) * 100)),
                currency: "NZD",
                effective_from: from,
              });
              toast.success("Tariff added");
              setTariffOpen(false);
            } catch {
              /* inline */
            }
          }}
        >
          <FormField
            label="Effective from"
            required
            error={fieldError(createTariff.error, "effective_from")}
          >
            <Input
              type="date"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="Unit"
            error={fieldError(createTariff.error, "unit")}
          >
            <Select value={unit} onChange={(e) => setUnit(e.target.value)}>
              {["per_night", "per_day", "per_item"].map((u) => (
                <option key={u} value={u}>
                  {u.replace("_", " ")}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField
            label="Price (NZ$)"
            required
            error={fieldError(createTariff.error, "price_cents")}
          >
            <Input
              type="number"
              min="0"
              step="0.01"
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              required
            />
          </FormField>
        </form>
      </Dialog>
    </div>
  );
}
