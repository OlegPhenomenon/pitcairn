import { toNum } from "../../lib/format";
export function CapacityCell({
  resource,
  date,
  used,
  capacity,
  unit,
  onClick,
}: {
  resource: string;
  date: string;
  used: bigint | number;
  capacity: bigint | number;
  unit: string;
  onClick: () => void;
}) {
  const n = toNum(used),
    max = toNum(capacity);
  return (
    <button
      aria-label={`${resource}, ${date}: ${n} of ${max} used`}
      title={`${date} · ${n}/${max} ${unit}`}
      className={`w-full rounded p-1.5 text-center hover:ring-2 hover:ring-teal-600 ${n >= max ? "bg-red-100 text-red-900" : n > 0 ? "bg-amber-100 text-amber-900" : "bg-green-50 text-green-800"}`}
      onClick={onClick}
    >
      {n}/{max}
    </button>
  );
}
