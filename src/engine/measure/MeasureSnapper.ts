import * as THREE from 'three';
import { snapNormal } from '../sectionplane/SectionPlane';
import { worldPerPixel } from '../sectionplane/SectionPlaneGizmo';
import { getFeatureLines } from './meshTopology';
import { objectWorldMatrix, type Entity } from './measureMath';

export type SnapKind = 'vertex' | 'midpoint' | 'edge' | 'face';

/** Which kinds of snap targets are enabled */
export type SnapSettings = Record<SnapKind, boolean>;

export interface SnapResult {
  kind: SnapKind;
  /** Snapped world position */
  point: THREE.Vector3;
  /** Edge the snap lies on (edge and midpoint snaps) */
  edge?: { a: THREE.Vector3; b: THREE.Vector3 };
  /** World normal of the face under the cursor, facing the camera */
  normal: THREE.Vector3;
  /** Object under the cursor */
  object: THREE.Mesh;
  instanceId?: number;
  faceIndex: number;
}

/** Snap radius around the cursor in screen pixels */
const SNAP_PIXELS = 12;

/**
 * Finds what is under the cursor: a corner, the midpoint of an edge, an edge
 * or a face — in that order of priority (like Trimble Connect).
 */
export class MeasureSnapper {
  private raycaster = new THREE.Raycaster();

  constructor(
    private getPickTargets: () => THREE.Object3D[],
    private getClippingPlanes: () => THREE.Plane[],
  ) {}

  /** Raycast the models: first visible, unclipped mesh hit. */
  pick(ndc: THREE.Vector2, camera: THREE.Camera): THREE.Intersection<THREE.Mesh> | null {
    this.raycaster.setFromCamera(ndc, camera);
    const hits = this.raycaster.intersectObjects(this.getPickTargets(), true);
    const clipping = this.getClippingPlanes();
    for (const hit of hits) {
      const obj = hit.object;
      if (!(obj instanceof THREE.Mesh) || !hit.face || hit.faceIndex == null) continue;
      if (!isVisible(obj)) continue;
      if (clipping.some((p) => p.distanceToPoint(hit.point) < 0)) continue;
      return hit as THREE.Intersection<THREE.Mesh>;
    }
    return null;
  }

  /**
   * @param ndc     cursor in normalized device coordinates
   * @param px      cursor in canvas pixels
   * @param size    canvas size in pixels
   */
  snap(
    ndc: THREE.Vector2, px: THREE.Vector2, size: THREE.Vector2,
    camera: THREE.Camera, settings: SnapSettings,
  ): SnapResult | null {
    const hit = this.pick(ndc, camera);
    if (!hit) return null;

    const mesh = hit.object;
    const matrix = objectWorldMatrix(mesh, hit.instanceId);
    const normal = hit.face!.normal.clone().applyMatrix3(new THREE.Matrix3().getNormalMatrix(matrix)).normalize();
    if (normal.dot(this.raycaster.ray.direction) > 0) normal.negate();
    snapNormal(normal);

    const base = { normal, object: mesh, instanceId: hit.instanceId, faceIndex: hit.faceIndex! };

    if (settings.vertex || settings.midpoint || settings.edge) {
      const edgeSnap = this.snapToEdges(hit.point, matrix, mesh.geometry, px, size, camera, settings);
      if (edgeSnap) return { ...base, ...edgeSnap };
    }
    if (settings.face) return { ...base, kind: 'face', point: hit.point.clone() };
    return null;
  }

  private snapToEdges(
    hitPoint: THREE.Vector3, matrix: THREE.Matrix4, geo: THREE.BufferGeometry,
    px: THREE.Vector2, size: THREE.Vector2, camera: THREE.Camera, settings: SnapSettings,
  ): Pick<SnapResult, 'kind' | 'point' | 'edge'> | null {
    const { lines, count } = getFeatureLines(geo);
    if (count === 0) return null;

    // Only look at edges near the hit point (cheap 3D test in local space)
    const radius = worldPerPixel(camera, hitPoint, size.y) * SNAP_PIXELS * 2;
    const localHit = hitPoint.clone().applyMatrix4(matrix.clone().invert());
    const localRadius2 = (radius / matrix.getMaxScaleOnAxis()) ** 2;

    const toScreen = (p: THREE.Vector3) => {
      const ndc = p.clone().project(camera);
      return new THREE.Vector2((ndc.x + 1) / 2 * size.x, (1 - ndc.y) / 2 * size.y);
    };

    const a = new THREE.Vector3(), b = new THREE.Vector3();
    const seg = new THREE.Line3();
    const closest = new THREE.Vector3();
    type Candidate = { d: number; p: THREE.Vector3; a: THREE.Vector3; b: THREE.Vector3 };
    let vertex = null as Candidate | null;
    let mid = null as Candidate | null;
    let edge = null as Candidate | null;

    for (let i = 0; i < count; i++) {
      a.fromArray(lines, i * 6);
      b.fromArray(lines, i * 6 + 3);
      seg.set(a, b).closestPointToPoint(localHit, true, closest);
      if (closest.distanceToSquared(localHit) > localRadius2) continue;

      const wa = a.clone().applyMatrix4(matrix);
      const wb = b.clone().applyMatrix4(matrix);
      const sa = toScreen(wa), sb = toScreen(wb);

      if (settings.vertex) {
        for (const [w, s] of [[wa, sa], [wb, sb]] as const) {
          const d = s.distanceTo(px);
          if (d < SNAP_PIXELS && (!vertex || d < vertex.d)) vertex = { d, p: w.clone(), a: wa, b: wb };
        }
      }
      if (settings.midpoint) {
        const d = sa.clone().add(sb).multiplyScalar(0.5).distanceTo(px);
        if (d < SNAP_PIXELS && (!mid || d < mid.d)) {
          mid = { d, p: wa.clone().add(wb).multiplyScalar(0.5), a: wa, b: wb };
        }
      }
      if (settings.edge) {
        const d = distanceToSegment2D(px, sa, sb);
        if (d < SNAP_PIXELS && (!edge || d < edge.d)) {
          const onEdge = new THREE.Vector3();
          this.raycaster.ray.distanceSqToSegment(wa, wb, undefined, onEdge);
          edge = { d, p: onEdge, a: wa, b: wb };
        }
      }
    }

    if (vertex) return { kind: 'vertex', point: vertex.p };
    if (mid) return { kind: 'midpoint', point: mid.p, edge: { a: mid.a, b: mid.b } };
    if (edge) return { kind: 'edge', point: edge.p, edge: { a: edge.a, b: edge.b } };
    return null;
  }
}

/** Entity a perpendicular measurement uses for this snap. */
export function snapToEntity(snap: SnapResult): Entity {
  switch (snap.kind) {
    case 'edge':
      return { kind: 'line', a: snap.edge!.a, b: snap.edge!.b, p: snap.point };
    case 'face':
      return { kind: 'plane', n: snap.normal, p: snap.point };
    default:
      return { kind: 'point', p: snap.point };
  }
}

function isVisible(obj: THREE.Object3D): boolean {
  for (let o: THREE.Object3D | null = obj; o; o = o.parent) {
    if (!o.visible) return false;
  }
  return true;
}

function distanceToSegment2D(p: THREE.Vector2, a: THREE.Vector2, b: THREE.Vector2): number {
  const ab = b.clone().sub(a);
  const len2 = ab.lengthSq();
  const t = len2 > 0 ? THREE.MathUtils.clamp(p.clone().sub(a).dot(ab) / len2, 0, 1) : 0;
  return a.clone().addScaledVector(ab, t).distanceTo(p);
}
