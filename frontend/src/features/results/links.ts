/** Deep link to one deliverable (analysis result) on the Results tab. */
export const deliverableHref = (projectId: string, deliverableId: string) =>
  `/app/projects/${projectId}/results?deliverable=${encodeURIComponent(deliverableId)}`;
