import { toNum } from "../../lib/format";
import { Banner, Skeleton } from "../../ui";
import { useDeliverableSamples } from "./api";

/**
 * Samples linked to one deliverable (§5 item 12) — readable by everyone who
 * can read the project.
 */
export function LinkedSamples({
  projectId,
  deliverableId,
}: {
  projectId: string;
  deliverableId: string;
}) {
  const samples = useDeliverableSamples(projectId, deliverableId);
  const linked = samples.data?.items ?? [];
  const total = toNum(samples.data?.total ?? 0);
  return (
    <section aria-label="Linked samples">
      <h3 className="font-semibold">Linked samples</h3>
      {samples.isPending ? (
        <Skeleton className="mt-2 h-10" />
      ) : samples.isError ? (
        <Banner tone="error">{samples.error.message}</Banner>
      ) : linked.length === 0 ? (
        <p className="mt-1 text-sm text-slate-600">
          No samples are linked to this result.
        </p>
      ) : (
        <ul className="mt-2 space-y-1 text-sm">
          {linked.map((s) => (
            <li key={s.id}>
              <strong>{s.code}</strong>
              {" · "}
              {s.material || "Material not recorded"}
              {" · "}
              {s.custodian_org
                ? `held by ${s.custodian_org}`
                : "Custodian not recorded"}
            </li>
          ))}
        </ul>
      )}
      {total > linked.length && (
        <p className="mt-1 text-sm text-slate-600">
          Showing {linked.length} of {total} linked samples — see the Samples
          tab for all.
        </p>
      )}
    </section>
  );
}
