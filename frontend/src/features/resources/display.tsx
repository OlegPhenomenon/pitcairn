import { formatDate, toNum } from "../../lib/format";

export const money = (cents: bigint | number, currency = "NZD") =>
  currency === "NZD"
    ? `NZ$${new Intl.NumberFormat("en-NZ", { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(toNum(cents) / 100)}`
    : new Intl.NumberFormat("en-NZ", { style: "currency", currency }).format(
        toNum(cents) / 100,
      );
export function DateText({ value }: { value: string | null | undefined }) {
  return (
    <time dateTime={value ?? undefined} title={value ?? undefined}>
      {formatDate(value)}
    </time>
  );
}
export const amountCents = (value: string) =>
  BigInt(Math.round(Number(value) * 100));
export const dateRange = (a: string, b: string) => (
  <>
    <DateText value={a} /> – <DateText value={b} />
  </>
);
