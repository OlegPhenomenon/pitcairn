import { formatDate } from "../../lib/format";

export function DateText({ value }: { value: string | null | undefined }) {
  return (
    <time dateTime={value ?? undefined} title={value ?? undefined}>
      {formatDate(value)}
    </time>
  );
}
export function DateRange({ start, end }: { start: string; end: string }) { return (
  <>
    <DateText value={start} /> – <DateText value={end} />
  </>
  );
}
