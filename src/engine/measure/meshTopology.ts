import * as THREE from 'three';

/**
 * Cached geometric analysis of mesh geometries for measuring:
 *
 * - Feature lines: the "real" edges of a model (sharp creases and open
 *   borders, not the diagonals of the triangulation). Collinear pieces are
 *   merged, so each line runs from corner to corner and its midpoint is the
 *   midpoint of the visible edge.
 * - Triangle topology: welded vertex ids and triangle neighbours, used to grow
 *   flat faces and to check whether a mesh is closed (for volumes).
 *
 * Everything is in the geometry's local space and computed lazily once per
 * geometry.
 */

/** Crease angle (degrees) above which a triangle edge counts as a model edge */
const FEATURE_ANGLE = 25;
/** Segments within this angle are merged into one line */
const COLLINEAR_COS = Math.cos(THREE.MathUtils.degToRad(0.5));
/** Vertices closer than this (local units) are welded together */
const WELD_PRECISION = 1e4;

export interface FeatureLines {
  /** Line endpoints as [ax, ay, az, bx, by, bz] per line */
  lines: Float32Array;
  count: number;
}

export interface Topology {
  triCount: number;
  /** Welded vertex id per triangle corner (triCount * 3) */
  corners: Uint32Array;
  /** Neighbour triangle across edge (corner i → corner i+1), -1 for open borders */
  neighbors: Int32Array;
  /** True when every edge is shared by exactly two triangles */
  closed: boolean;
}

const featureCache = new WeakMap<THREE.BufferGeometry, FeatureLines>();
const topologyCache = new WeakMap<THREE.BufferGeometry, Topology>();

function weldKey(x: number, y: number, z: number): string {
  return `${Math.round(x * WELD_PRECISION)},${Math.round(y * WELD_PRECISION)},${Math.round(z * WELD_PRECISION)}`;
}

/** Vertex index of triangle corner `c` (honours the index buffer). */
export function cornerIndex(geo: THREE.BufferGeometry, c: number): number {
  return geo.index ? geo.index.getX(c) : c;
}

export function triangleCount(geo: THREE.BufferGeometry): number {
  const n = geo.index ? geo.index.count : geo.getAttribute('position').count;
  return Math.floor(n / 3);
}

// ============================================================================
// Feature lines
// ============================================================================

export function getFeatureLines(geo: THREE.BufferGeometry): FeatureLines {
  const cached = featureCache.get(geo);
  if (cached) return cached;

  const edges = new THREE.EdgesGeometry(geo, FEATURE_ANGLE);
  const segs = edges.getAttribute('position').array as Float32Array;
  const result = mergeCollinear(segs);
  edges.dispose();

  featureCache.set(geo, result);
  return result;
}

/**
 * Merge chains of collinear segments into single lines. A chain also runs
 * through T-junctions, so a long edge stays one line when another edge
 * touches it halfway.
 */
function mergeCollinear(segs: Float32Array): FeatureLines {
  const segCount = segs.length / 6;
  const ids = new Map<string, number>();
  const verts: number[] = [];
  const ends = new Uint32Array(segCount * 2);

  const vertexId = (o: number) => {
    const key = weldKey(segs[o], segs[o + 1], segs[o + 2]);
    let id = ids.get(key);
    if (id === undefined) {
      id = verts.length / 3;
      ids.set(key, id);
      verts.push(segs[o], segs[o + 1], segs[o + 2]);
    }
    return id;
  };
  for (let s = 0; s < segCount; s++) {
    ends[s * 2] = vertexId(s * 6);
    ends[s * 2 + 1] = vertexId(s * 6 + 3);
  }

  const adjacency: number[][] = Array.from({ length: verts.length / 3 }, () => []);
  for (let s = 0; s < segCount; s++) {
    if (ends[s * 2] === ends[s * 2 + 1]) continue;
    adjacency[ends[s * 2]].push(s);
    adjacency[ends[s * 2 + 1]].push(s);
  }

  const dir = new THREE.Vector3();
  const other = new THREE.Vector3();
  const segDir = (s: number, out: THREE.Vector3) => {
    const a = ends[s * 2] * 3, b = ends[s * 2 + 1] * 3;
    return out.set(verts[b] - verts[a], verts[b + 1] - verts[a + 1], verts[b + 2] - verts[a + 2]).normalize();
  };

  const visited = new Uint8Array(segCount);
  /** Walk from vertex `v` along `dir` as long as a collinear segment continues */
  const extend = (v: number): number => {
    for (;;) {
      let next = -1;
      for (const t of adjacency[v]) {
        if (visited[t]) continue;
        if (Math.abs(segDir(t, other).dot(dir)) > COLLINEAR_COS) { next = t; break; }
      }
      if (next < 0) return v;
      visited[next] = 1;
      v = ends[next * 2] === v ? ends[next * 2 + 1] : ends[next * 2];
    }
  };

  const out: number[] = [];
  for (let s = 0; s < segCount; s++) {
    if (visited[s] || ends[s * 2] === ends[s * 2 + 1]) continue;
    visited[s] = 1;
    segDir(s, dir);
    const a = extend(ends[s * 2]) * 3;
    const b = extend(ends[s * 2 + 1]) * 3;
    out.push(verts[a], verts[a + 1], verts[a + 2], verts[b], verts[b + 1], verts[b + 2]);
  }
  return { lines: new Float32Array(out), count: out.length / 6 };
}

// ============================================================================
// Triangle topology
// ============================================================================

export function getTopology(geo: THREE.BufferGeometry): Topology {
  const cached = topologyCache.get(geo);
  if (cached) return cached;

  const pos = geo.getAttribute('position');
  const triCount = triangleCount(geo);
  const corners = new Uint32Array(triCount * 3);
  const ids = new Map<string, number>();
  for (let c = 0; c < triCount * 3; c++) {
    const i = cornerIndex(geo, c);
    const key = weldKey(pos.getX(i), pos.getY(i), pos.getZ(i));
    let id = ids.get(key);
    if (id === undefined) { id = ids.size; ids.set(key, id); }
    corners[c] = id;
  }

  // Pair up triangles over shared (undirected) edges
  const vertexCount = ids.size;
  const firstOwner = new Map<number, number>();
  const edgeUse = new Map<number, number>();
  const neighbors = new Int32Array(triCount * 3).fill(-1);
  for (let t = 0; t < triCount; t++) {
    for (let e = 0; e < 3; e++) {
      const a = corners[t * 3 + e];
      const b = corners[t * 3 + ((e + 1) % 3)];
      if (a === b) continue;
      const key = Math.min(a, b) * vertexCount + Math.max(a, b);
      edgeUse.set(key, (edgeUse.get(key) ?? 0) + 1);
      const owner = firstOwner.get(key);
      if (owner === undefined) {
        firstOwner.set(key, t * 3 + e);
      } else if (neighbors[owner] === -1) {
        neighbors[owner] = t;
        neighbors[t * 3 + e] = Math.floor(owner / 3);
      }
    }
  }

  let closed = edgeUse.size > 0;
  for (const n of edgeUse.values()) {
    if (n !== 2) { closed = false; break; }
  }

  const topology: Topology = { triCount, corners, neighbors, closed };
  topologyCache.set(geo, topology);
  return topology;
}
