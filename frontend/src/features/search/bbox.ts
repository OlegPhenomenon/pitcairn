/** `[minLng, minLat, maxLng, maxLat]`, the order the search API expects. */
export type Bbox = [number, number, number, number];

/** Degrees added around a single point so the area search has some extent (~1 km). */
export const POINT_BUFFER_DEG = 0.01;

/** Parses the `bbox` search param; returns null when absent or malformed. */
export function parseBbox(raw: string | null | undefined): Bbox | null {
  if (!raw) return null;
  const parts = raw.split(',');
  if (parts.length !== 4 || parts.some(p => p.trim() === '')) return null;
  const nums = parts.map(Number);
  if (nums.some(n => !Number.isFinite(n))) return null;
  const [minLng, minLat, maxLng, maxLat] = nums;
  if (minLng > maxLng || minLat > maxLat) return null;
  return [minLng, minLat, maxLng, maxLat];
}

export function formatBbox(bbox: Bbox): string {
  return bbox.map(n => Number(n.toFixed(6))).join(',');
}

export function geometryBbox(geometry: { type?: unknown; coordinates?: unknown }): Bbox | null {
  const c = geometry.coordinates;
  if (geometry.type === 'Point' && Array.isArray(c) && c.length >= 2) {
    const lng = Number(c[0]);
    const lat = Number(c[1]);
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
    return [lng - POINT_BUFFER_DEG, lat - POINT_BUFFER_DEG, lng + POINT_BUFFER_DEG, lat + POINT_BUFFER_DEG];
  }
  if (geometry.type === 'Polygon' && Array.isArray(c) && Array.isArray(c[0])) {
    const ring = (c[0] as unknown[]).filter((p): p is number[] => Array.isArray(p) && p.length >= 2);
    if (ring.length === 0) return null;
    const lngs = ring.map(p => Number(p[0]));
    const lats = ring.map(p => Number(p[1]));
    return [Math.min(...lngs), Math.min(...lats), Math.max(...lngs), Math.max(...lats)];
  }
  return null;
}
