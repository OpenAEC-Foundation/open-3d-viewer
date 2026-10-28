import * as THREE from 'three';
import { cornerIndex, getTopology, triangleCount } from './meshTopology';

/**
 * Pure geometry for the measure tools. All inputs and outputs are in world space.
 */

/** Something a measurement can start or end on */
export type Entity =
  | { kind: 'point'; p: THREE.Vector3 }
  /** Edge from a to b; p is where it was picked */
  | { kind: 'line'; a: THREE.Vector3; b: THREE.Vector3; p: THREE.Vector3 }
  /** Flat face through p with unit normal n */
  | { kind: 'plane'; n: THREE.Vector3; p: THREE.Vector3 };

export interface DistanceResult {
  p1: THREE.Vector3;
  p2: THREE.Vector3;
  /** Dashed helper lines where a foot point lies on the extension of an edge */
  extensions: [THREE.Vector3, THREE.Vector3][];
}

const PARALLEL_EPS = 1e-9;

function footOnLine(p: THREE.Vector3, a: THREE.Vector3, b: THREE.Vector3): THREE.Vector3 {
  const d = b.clone().sub(a);
  const len2 = d.lengthSq();
  if (len2 < PARALLEL_EPS) return a.clone();
  return a.clone().addScaledVector(d, p.clone().sub(a).dot(d) / len2);
}

function footOnPlane(p: THREE.Vector3, n: THREE.Vector3, origin: THREE.Vector3): THREE.Vector3 {
  return p.clone().addScaledVector(n, -p.clone().sub(origin).dot(n));
}

/**
 * Closest points between two infinite lines, or null when they are parallel.
 */
function closestBetweenLines(
  a1: THREE.Vector3, b1: THREE.Vector3, a2: THREE.Vector3, b2: THREE.Vector3,
): [THREE.Vector3, THREE.Vector3] | null {
  const d1 = b1.clone().sub(a1);
  const d2 = b2.clone().sub(a2);
  const r = a1.clone().sub(a2);
  const a = d1.dot(d1), e = d2.dot(d2), b = d1.dot(d2);
  const c = d1.dot(r), f = d2.dot(r);
  const denom = a * e - b * b;
  if (denom < PARALLEL_EPS * a * e || a < PARALLEL_EPS || e < PARALLEL_EPS) return null;
  const s = (b * f - c * e) / denom;
  const t = (a * f - b * c) / denom;
  return [a1.clone().addScaledVector(d1, s), a2.clone().addScaledVector(d2, t)];
}

/** Helper line from the nearest endpoint of an edge to a point on its extension. */
function extensionTo(line: Extract<Entity, { kind: 'line' }>, p: THREE.Vector3): [THREE.Vector3, THREE.Vector3] | null {
  const d = line.b.clone().sub(line.a);
  const len2 = d.lengthSq();
  if (len2 < PARALLEL_EPS) return null;
  const t = p.clone().sub(line.a).dot(d) / len2;
  const tol = 1e-6;
  if (t < -tol) return [line.a.clone(), p.clone()];
  if (t > 1 + tol) return [line.b.clone(), p.clone()];
  return null;
}

/**
 * Perpendicular measurement between two entities (as in Trimble Connect):
 * - to or from a face: perpendicular to that face (the first face wins)
 * - to or from an edge: perpendicular to that edge
 * - edge to edge: the common perpendicular (shortest distance between the lines)
 * - point to point: straight distance
 */
export function perpendicularDistance(A: Entity, B: Entity): DistanceResult {
  let p1: THREE.Vector3;
  let p2: THREE.Vector3;

  if (A.kind === 'plane') {
    p2 = B.p.clone();
    p1 = footOnPlane(B.p, A.n, A.p);
  } else if (B.kind === 'plane') {
    p1 = A.p.clone();
    p2 = footOnPlane(A.p, B.n, B.p);
  } else if (A.kind === 'line' && B.kind === 'line') {
    const closest = closestBetweenLines(A.a, A.b, B.a, B.b);
    if (closest) {
      [p1, p2] = closest;
    } else {
      p2 = B.p.clone();
      p1 = footOnLine(B.p, A.a, A.b);
    }
  } else if (A.kind === 'line') {
    p2 = B.p.clone();
    p1 = footOnLine(B.p, A.a, A.b);
  } else if (B.kind === 'line') {
    p1 = A.p.clone();
    p2 = footOnLine(A.p, B.a, B.b);
  } else {
    p1 = A.p.clone();
    p2 = B.p.clone();
  }

  const extensions: [THREE.Vector3, THREE.Vector3][] = [];
  if (A.kind === 'line') { const ext = extensionTo(A, p1); if (ext) extensions.push(ext); }
  if (B.kind === 'line') { const ext = extensionTo(B, p2); if (ext) extensions.push(ext); }
  return { p1, p2, extensions };
}

// ============================================================================
// Mesh helpers
// ============================================================================

/** World matrix of a hit object, including the instance transform of instanced meshes. */
export function objectWorldMatrix(object: THREE.Mesh, instanceId?: number): THREE.Matrix4 {
  const m = object.matrixWorld.clone();
  if (object instanceof THREE.InstancedMesh && instanceId !== undefined) {
    const inst = new THREE.Matrix4();
    object.getMatrixAt(instanceId, inst);
    m.multiply(inst);
  }
  return m;
}

function readCorner(geo: THREE.BufferGeometry, c: number, matrix: THREE.Matrix4, out: THREE.Vector3): THREE.Vector3 {
  return out.fromBufferAttribute(geo.getAttribute('position') as THREE.BufferAttribute, cornerIndex(geo, c)).applyMatrix4(matrix);
}

// ============================================================================
// Face area (flat or curved)
// ============================================================================

export interface FaceRegion {
  /** World-space triangles of the face (9 floats per triangle) */
  triangles: Float32Array;
  /** Outline segments (6 floats per segment) */
  outline: Float32Array;
  /** Triangle indices of the face within the mesh */
  faces: Set<number>;
  area: number;
  perimeter: number;
  /** Area-weighted centre */
  centroid: THREE.Vector3;
  normal: THREE.Vector3;
  /** True for a curved surface (e.g. the side of a cylinder) */
  curved: boolean;
}

/** Faces with more triangles than this are cut off (keeps the UI responsive) */
const MAX_REGION_TRIANGLES = 500_000;
/** Neighbours bending less than this belong to the same (smooth) surface; matches the feature-edge angle */
const SMOOTH_COS = Math.cos(THREE.MathUtils.degToRad(25));
/** Normals within this angle count as flat */
const FLAT_COS = Math.cos(THREE.MathUtils.degToRad(1));

/**
 * Grow the face around triangle `faceIndex`: all connected triangles up to the
 * model edges (creases sharper than 25°). For a flat face this is the plane;
 * for a tessellated curved surface (cylinder, dome) the whole smooth surface,
 * so its true area is measured instead of a single facet.
 */
export function flatFaceRegion(mesh: THREE.Mesh, faceIndex: number, instanceId?: number): FaceRegion {
  const geo = mesh.geometry;
  const topo = getTopology(geo);
  const matrix = objectWorldMatrix(mesh, instanceId);

  const tri = (t: number) => [0, 1, 2].map((k) => readCorner(geo, t * 3 + k, matrix, new THREE.Vector3()));
  const triNormal = (v: THREE.Vector3[]) =>
    new THREE.Vector3().crossVectors(v[1].clone().sub(v[0]), v[2].clone().sub(v[0]));

  const normal = triNormal(tri(faceIndex)).normalize();
  const inRegion = new Uint8Array(topo.triCount);
  const queue = [faceIndex];
  inRegion[faceIndex] = 1;
  const triangles: number[] = [];
  const outline: number[] = [];
  const faces = new Set<number>();
  let area = 0;
  let perimeter = 0;
  let curved = false;
  const centroid = new THREE.Vector3();

  while (queue.length > 0 && faces.size < MAX_REGION_TRIANGLES) {
    const t = queue.pop()!;
    faces.add(t);
    const v = tri(t);
    const n = triNormal(v);
    const a = n.length() / 2;
    if (a > 0) n.normalize();
    area += a;
    centroid.addScaledVector(v[0].clone().add(v[1]).add(v[2]), a / 3);
    for (const p of v) triangles.push(p.x, p.y, p.z);
    if (a > 0 && n.dot(normal) < FLAT_COS) curved = true;

    for (let e = 0; e < 3; e++) {
      const nb = topo.neighbors[t * 3 + e];
      let border = nb < 0;
      if (!border && !inRegion[nb]) {
        const nn = triNormal(tri(nb));
        const len = nn.length();
        // Degenerate slivers are taken along; otherwise the bend must be smooth
        if (len === 0 || a === 0 || nn.dot(n) / len > SMOOTH_COS) {
          inRegion[nb] = 1;
          queue.push(nb);
        } else {
          border = true;
        }
      }
      if (border) {
        const p = v[e], q = v[(e + 1) % 3];
        outline.push(p.x, p.y, p.z, q.x, q.y, q.z);
        perimeter += p.distanceTo(q);
      }
    }
  }

  if (area > 0) centroid.divideScalar(area);
  // Anchor the label on the surface (the centroid of a curved face lies inside it)
  if (curved) centroid.copy(tri(faceIndex).reduce((s, p) => s.add(p), new THREE.Vector3()).divideScalar(3));
  return {
    triangles: new Float32Array(triangles),
    outline: new Float32Array(outline),
    faces, area, perimeter, centroid, normal, curved,
  };
}

// ============================================================================
// Volume and total surface area
// ============================================================================

export interface VolumeResult {
  volume: number;
  surfaceArea: number;
  /** Whether all meshes are closed; the volume is only exact for closed meshes */
  closed: boolean;
  /** Centre of the world bounding box (label anchor) */
  center: THREE.Vector3;
}

/**
 * Volume of an object (one or more meshes) as the sum of signed tetrahedra
 * (divergence theorem), plus its total surface area.
 */
export function objectVolume(parts: { mesh: THREE.Mesh; instanceId?: number }[]): VolumeResult {
  const a = new THREE.Vector3(), b = new THREE.Vector3(), c = new THREE.Vector3();
  const ab = new THREE.Vector3(), ac = new THREE.Vector3();
  const box = new THREE.Box3();
  let volume = 0;
  let surfaceArea = 0;
  let closed = parts.length > 0;

  for (const { mesh, instanceId } of parts) {
    const geo = mesh.geometry;
    const matrix = objectWorldMatrix(mesh, instanceId);
    const triCount = triangleCount(geo);
    let partVolume = 0;
    for (let t = 0; t < triCount; t++) {
      readCorner(geo, t * 3, matrix, a);
      readCorner(geo, t * 3 + 1, matrix, b);
      readCorner(geo, t * 3 + 2, matrix, c);
      partVolume += a.dot(ab.crossVectors(b, c)) / 6;
      surfaceArea += ab.subVectors(b, a).cross(ac.subVectors(c, a)).length() / 2;
      box.expandByPoint(a).expandByPoint(b).expandByPoint(c);
    }
    // Each part may be wound either way
    volume += Math.abs(partVolume);
    if (!getTopology(geo).closed) closed = false;
  }

  return { volume, surfaceArea, closed, center: box.getCenter(new THREE.Vector3()) };
}

// ============================================================================
// Polygon area
// ============================================================================

export interface PolygonResult {
  area: number;
  perimeter: number;
  normal: THREE.Vector3;
  centroid: THREE.Vector3;
  /** Triangle indices into the input points, for a fill */
  triangles: number[];
}

/**
 * Area of a closed polygon through 3D points (Newell's method). For points
 * that are not exactly coplanar this is the area projected on the best-fit plane.
 */
export function polygonArea(points: THREE.Vector3[]): PolygonResult {
  const normal = new THREE.Vector3();
  const centroid = new THREE.Vector3();
  let perimeter = 0;
  for (let i = 0; i < points.length; i++) {
    const p = points[i], q = points[(i + 1) % points.length];
    normal.x += (p.y - q.y) * (p.z + q.z);
    normal.y += (p.z - q.z) * (p.x + q.x);
    normal.z += (p.x - q.x) * (p.y + q.y);
    perimeter += p.distanceTo(q);
    centroid.add(p);
  }
  const area = normal.length() / 2;
  if (points.length > 0) centroid.divideScalar(points.length);
  if (area > 0) normal.normalize(); else normal.set(0, 0, 1);

  // Triangulate in the polygon's own plane
  const ref = Math.abs(normal.z) < 0.9 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(1, 0, 0);
  const u = new THREE.Vector3().crossVectors(ref, normal).normalize();
  const v = new THREE.Vector3().crossVectors(normal, u);
  const contour = points.map((p) => new THREE.Vector2(p.dot(u), p.dot(v)));
  const triangles = points.length >= 3 && area > 0
    ? THREE.ShapeUtils.triangulateShape(contour, []).flat()
    : [];

  return { area, perimeter, normal, centroid, triangles };
}

// ============================================================================
// Angles
// ============================================================================

export interface AngleResult {
  /** Angle in radians (0..π) */
  angle: number;
  /** Apex of the angle */
  center: THREE.Vector3;
  /** Unit directions of the two legs: from the first entity (v1) to the second (v2) */
  v1: THREE.Vector3;
  v2: THREE.Vector3;
  /** Leg lengths (to where the entities were picked) */
  len1: number;
  len2: number;
}

const ANGLE_EPS = 1e-6;

/** Any unit vector perpendicular to `d` */
function anyPerpendicular(d: THREE.Vector3): THREE.Vector3 {
  const ref = Math.abs(d.z) < 0.9 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(1, 0, 0);
  return new THREE.Vector3().crossVectors(d, ref).normalize();
}

/**
 * Direction along an edge from `from` towards where it was picked (or towards
 * its far end when picked right at `from`). Returns the direction and leg length.
 */
function edgeLeg(line: Extract<Entity, { kind: 'line' }>, from: THREE.Vector3): [THREE.Vector3, number] {
  const toPick = line.p.clone().sub(from);
  if (toPick.length() > ANGLE_EPS * (1 + line.a.distanceTo(line.b))) {
    const d = line.b.clone().sub(line.a).normalize();
    if (toPick.dot(d) < 0) d.negate();
    return [d, toPick.length()];
  }
  const far = from.distanceTo(line.a) > from.distanceTo(line.b) ? line.a : line.b;
  return [far.clone().sub(from).normalize(), far.distanceTo(from)];
}

/** Leg in a plane, perpendicular to `axis`, towards the picked point */
function planeLeg(plane: Extract<Entity, { kind: 'plane' }>, from: THREE.Vector3, axis: THREE.Vector3): [THREE.Vector3, number] {
  const toPick = plane.p.clone().sub(from);
  toPick.addScaledVector(axis, -toPick.dot(axis));
  const len = toPick.length();
  if (len > ANGLE_EPS) return [toPick.divideScalar(len), len];
  return [new THREE.Vector3().crossVectors(plane.n, axis).normalize(), 0];
}

function result(center: THREE.Vector3, v1: THREE.Vector3, len1: number, v2: THREE.Vector3, len2: number): AngleResult {
  return { angle: v1.angleTo(v2), center, v1, v2, len1, len2 };
}

/**
 * Angle between two edges and/or faces, measured from A to B:
 * - face–face: the angle between the two faces at their intersection line
 * - edge–edge: the angle between the edges at their (closest) crossing point
 * - edge–face: the angle between the edge and the face
 * Returns null for points (an angle needs edges or faces) or parallel entities.
 */
export function angleBetween(A: Entity, B: Entity): AngleResult | null {
  if (A.kind === 'point' || B.kind === 'point') return null;

  if (A.kind === 'plane' && B.kind === 'plane') {
    const axis = new THREE.Vector3().crossVectors(A.n, B.n);
    if (axis.length() < ANGLE_EPS) return null;
    axis.normalize();
    // A point on the intersection line of both planes
    const c = A.n.dot(B.n);
    const h1 = A.n.dot(A.p), h2 = B.n.dot(B.p);
    const origin = A.n.clone().multiplyScalar((h1 - h2 * c) / (1 - c * c))
      .addScaledVector(B.n, (h2 - h1 * c) / (1 - c * c));
    // Apex on the intersection line, between the two picks
    const mid = A.p.clone().add(B.p).multiplyScalar(0.5);
    const center = origin.addScaledVector(axis, mid.clone().sub(origin).dot(axis));
    const [v1, len1] = planeLeg(A, center, axis);
    const [v2, len2] = planeLeg(B, center, axis);
    return result(center, v1, len1, v2, len2);
  }

  if (A.kind === 'line' && B.kind === 'line') {
    const closest = closestBetweenLines(A.a, A.b, B.a, B.b);
    if (!closest) return null;
    const [cA, cB] = closest;
    const [v1, len1] = edgeLeg(A, cA);
    const [v2, len2] = edgeLeg(B, cB);
    return result(cA.clone().add(cB).multiplyScalar(0.5), v1, len1, v2, len2);
  }

  // Edge and face (either order)
  const line = (A.kind === 'line' ? A : B) as Extract<Entity, { kind: 'line' }>;
  const plane = (A.kind === 'plane' ? A : B) as Extract<Entity, { kind: 'plane' }>;
  const d = line.b.clone().sub(line.a).normalize();
  const denom = plane.n.dot(d);
  if (Math.abs(denom) < ANGLE_EPS) return null;
  const center = line.a.clone().addScaledVector(d, plane.n.dot(plane.p.clone().sub(line.a)) / denom);
  const [edgeDir, edgeLen] = edgeLeg(line, center);
  // Leg in the face: the edge projected onto the face
  let faceDir = edgeDir.clone().addScaledVector(plane.n, -edgeDir.dot(plane.n));
  let faceLen = plane.p.distanceTo(center);
  if (faceDir.length() < ANGLE_EPS) {
    // Edge perpendicular to the face: draw the face leg towards where the face was picked
    faceDir = plane.p.clone().sub(center).addScaledVector(plane.n, -plane.p.clone().sub(center).dot(plane.n));
    if (faceDir.length() < ANGLE_EPS) faceDir = anyPerpendicular(plane.n);
    faceDir.normalize();
  } else {
    faceDir.normalize();
  }
  return A.kind === 'line'
    ? result(center, edgeDir, edgeLen, faceDir, faceLen)
    : result(center, faceDir, faceLen, edgeDir, edgeLen);
}

/** Angle ABC at vertex B through three points */
export function angleThreePoints(a: THREE.Vector3, b: THREE.Vector3, c: THREE.Vector3): AngleResult | null {
  const v1 = a.clone().sub(b), v2 = c.clone().sub(b);
  const len1 = v1.length(), len2 = v2.length();
  if (len1 < ANGLE_EPS || len2 < ANGLE_EPS) return null;
  return result(b.clone(), v1.divideScalar(len1), len1, v2.divideScalar(len2), len2);
}
