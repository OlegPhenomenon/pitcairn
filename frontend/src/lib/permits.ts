import type { DecisionDto } from '../api/generated/DecisionDto';
import { decisionKindLabel } from './decisionKind';

/** Display name of a permit chain decision: its title, else its kind. */
export const permitName = (d: Pick<DecisionDto, 'title' | 'kind'>): string => d.title || decisionKindLabel(d.kind);

/** Current head of every permit chain (issued, not superseded), revoked chains included. */
export const permitChainHeads = (items: DecisionDto[]): DecisionDto[] =>
  items.filter(d => d.status === 'issued' && !d.superseded_by_id && d.chain_id);

/** Permits currently in force: chain heads that are not revocations. */
export const permitsInForce = (items: DecisionDto[]): DecisionDto[] =>
  permitChainHeads(items).filter(d => d.kind !== 'revocation');

/** The refusal of the application, if one was issued. */
export const issuedRefusal = (items: DecisionDto[]): DecisionDto | undefined =>
  items.find(d => d.status === 'issued' && d.kind === 'refusal');
