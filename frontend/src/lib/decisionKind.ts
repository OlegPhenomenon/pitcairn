export const decisionKindLabel = (kind: string): string => {
  const superseded = kind.endsWith(' (superseded)');
  const bare = superseded ? kind.slice(0, -13) : kind;
  const label = ({
  permit: 'Research permit',
  refusal: 'Refusal',
  amendment: 'Amendment',
  extension: 'Extension',
  revocation: 'Revocation',
  })[bare] ?? bare;
  return superseded ? `${label} (superseded)` : label;
};
