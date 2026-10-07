import { describe, expect, it } from 'vitest';
import { formatBbox, geometryBbox, parseBbox, POINT_BUFFER_DEG } from './bbox';

describe('search area bbox', () => {
  it('round-trips the URL bbox param and rejects malformed values', () => {
    expect(parseBbox('-130.2,-25.1,-130,-25')).toEqual([-130.2, -25.1, -130, -25]);
    expect(formatBbox([-130.2, -25.1, -130, -25])).toBe('-130.2,-25.1,-130,-25');
    for (const bad of [null, '', '1,2,3', '1,2,x,4', '5,0,1,1', '1,,2,3']) expect(parseBbox(bad)).toBeNull();
  });

  it('derives a site area from polygon bounds and buffers points', () => {
    expect(geometryBbox({ type: 'Polygon', coordinates: [[[-130.1, -25.07], [-130.08, -25.07], [-130.08, -25.05], [-130.1, -25.07]]] }))
      .toEqual([-130.1, -25.07, -130.08, -25.05]);
    expect(geometryBbox({ type: 'Point', coordinates: [-130.1, -25.06] }))
      .toEqual([-130.1 - POINT_BUFFER_DEG, -25.06 - POINT_BUFFER_DEG, -130.1 + POINT_BUFFER_DEG, -25.06 + POINT_BUFFER_DEG]);
  });
});
