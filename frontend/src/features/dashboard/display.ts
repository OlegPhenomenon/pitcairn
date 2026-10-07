export const moneyText = (value: string) =>
  value.replace(
    /NZD (\d+(?:\.\d+)?)/g,
    (_, amount: string) =>
      `NZ$${new Intl.NumberFormat("en-NZ", { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(Number(amount))}`,
  );
