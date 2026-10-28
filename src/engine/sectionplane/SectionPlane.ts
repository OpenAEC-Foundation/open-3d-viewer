import * as THREE from 'three';
import { SectionPlaneGizmo } from './SectionPlaneGizmo';
import type { SectionPlaneMode } from '../../state/slices/viewerSlice';

/** Components smaller than this (~0.5°) are snapped to zero */
const SNAP_EPSILON = Math.sin(THREE.MathUtils.degToRad(0.5));

/**
 * Snap an (almost) axis-aligned or (almost) vertical normal to exact values,
 * so walls and floors give clean orthogonal sections.
 */
export function snapNormal(normal: THREE.Vector3): THREE.Vector3 {
  if (Math.abs(normal.x) < SNAP_EPSILON) normal.x = 0;
  if (Math.abs(normal.y) < SNAP_EPSILON) normal.y = 0;
  if (Math.abs(normal.z) < SNAP_EPSILON) normal.z = 0;
  return normal.normalize();
}

/**
 * Right-handed in-plane axes for a normal. For vertical and sloped planes
 * `u` is horizontal and `v` points upwards (Z-up), which keeps the plate
 * aligned with walls and roofs.
 */
function planeBasis(normal: THREE.Vector3): { u: THREE.Vector3; v: THREE.Vector3 } {
  const ref = Math.abs(normal.z) < 0.99 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(1, 0, 0);
  const u = new THREE.Vector3().crossVectors(ref, normal).normalize();
  const v = new THREE.Vector3().crossVectors(normal, u).normalize();
  return { u, v };
}

/**
 * Single clipping plane placed by clicking on a face of the model.
 *
 * While picking, a preview plate follows the face under the mouse. After
 * placing, everything on the side the face was facing (towards the viewer)
 * is cut away and the plane can be dragged along its normal.
 */
export class SectionPlane {
  /** Clipping plane — geometry with negative distance (where the arrow points) is cut away */
  readonly plane = new THREE.Plane(new THREE.Vector3(0, 0, -1), 0);
  mode: SectionPlaneMode = 'off';
  /** Whether a plane was placed since the tool was switched on */
  hasPlane = false;

  readonly gizmo = new SectionPlaneGizmo();
  private scene: THREE.Scene;
  private getSceneBounds: () => THREE.Box3;
  /** Cached scene bounds (recomputed after invalidateBounds) */
  private bounds: THREE.Box3 | null = null;

  constructor(scene: THREE.Scene, getSceneBounds: () => THREE.Box3) {
    this.scene = scene;
    this.getSceneBounds = getSceneBounds;
  }

  /** Call when models are added or removed. */
  invalidateBounds(): void {
    this.bounds = null;
    if (this.mode === 'placed') this.updateVisual();
  }

  setMode(mode: SectionPlaneMode): void {
    if (mode === 'placed' && !this.hasPlane) mode = 'picking';
    this.mode = mode;
    if (mode === 'off') this.hasPlane = false;
    this.updateVisual();
  }

  /** Planes that should currently clip the scene. */
  getActivePlanes(): THREE.Plane[] {
    return this.mode === 'placed' ? [this.plane] : [];
  }

  /**
   * Show the hover preview on the face under the mouse.
   */
  showPreview(point: THREE.Vector3, faceNormal: THREE.Vector3): void {
    if (this.mode !== 'picking') return;
    const bounds = this.getBounds();
    const diag = bounds.getSize(new THREE.Vector3()).length();
    const size = Math.max(diag * 0.12, 0.5);
    const { u, v } = planeBasis(faceNormal);

    this.gizmo.update({
      center: point.clone().addScaledVector(faceNormal, this.epsilon(diag)),
      u, v,
      normal: faceNormal,
      width: size,
      height: size,
      arrowDir: faceNormal,
      preview: true,
    });
    this.attach();
  }

  hidePreview(): void {
    if (this.mode === 'picking') this.gizmo.clear();
  }

  /**
   * Place the section plane on a face. The side the face points to is cut away.
   */
  place(point: THREE.Vector3, faceNormal: THREE.Vector3): void {
    const diag = this.getBounds().getSize(new THREE.Vector3()).length();
    const onFace = point.clone().addScaledVector(faceNormal, this.epsilon(diag));
    this.plane.setFromNormalAndCoplanarPoint(faceNormal.clone().negate(), onFace);
    this.hasPlane = true;
    this.mode = 'placed';
    this.updateVisual();
  }

  /** Cut away the other side instead. */
  flip(): void {
    if (!this.hasPlane) return;
    this.plane.negate();
    this.updateVisual();
  }

  /** Position of the plane along its normal. */
  getOffset(): number {
    return -this.plane.constant;
  }

  /** Move the plane along its normal, clamped to just outside the model. */
  setOffset(offset: number): void {
    const [lo, hi] = this.getOffsetRange();
    this.plane.constant = -THREE.MathUtils.clamp(offset, lo, hi);
    this.updateVisual();
  }

  getHandlePosition(): THREE.Vector3 {
    return this.gizmo.getHandlePosition();
  }

  /**
   * Rebuild the visuals (e.g. after the plane moved or the model bounds changed).
   */
  updateVisual(): void {
    if (this.mode === 'off') {
      this.gizmo.clear();
      this.detach();
      return;
    }
    if (this.mode === 'picking') {
      this.gizmo.clear();
      this.attach();
      return;
    }

    // Fit a rectangle on the plane around the projected model bounds
    const normal = this.plane.normal;
    const { u, v } = planeBasis(normal);
    const bounds = this.getBounds();
    let minU = Infinity, maxU = -Infinity, minV = Infinity, maxV = -Infinity;
    for (const corner of this.getCorners(bounds)) {
      const du = corner.dot(u);
      const dv = corner.dot(v);
      minU = Math.min(minU, du); maxU = Math.max(maxU, du);
      minV = Math.min(minV, dv); maxV = Math.max(maxV, dv);
    }
    const width = Math.max((maxU - minU) * 1.1, 0.1);
    const height = Math.max((maxV - minV) * 1.1, 0.1);
    const center = normal.clone().multiplyScalar(-this.plane.constant)
      .addScaledVector(u, (minU + maxU) / 2)
      .addScaledVector(v, (minV + maxV) / 2);

    this.gizmo.update({
      center, u, v, normal, width, height,
      arrowDir: normal.clone().negate(),
      preview: false,
    });
    this.attach();
  }

  dispose(): void {
    this.detach();
    this.gizmo.dispose();
  }

  // ============================================================================
  // Private
  // ============================================================================

  private getBounds(): THREE.Box3 {
    if (!this.bounds) {
      const bounds = this.getSceneBounds();
      this.bounds = bounds.isEmpty()
        ? new THREE.Box3(new THREE.Vector3(-5, -5, -5), new THREE.Vector3(5, 5, 5))
        : bounds;
    }
    return this.bounds;
  }

  private getCorners(box: THREE.Box3): THREE.Vector3[] {
    const { min, max } = box;
    const corners: THREE.Vector3[] = [];
    for (const x of [min.x, max.x]) {
      for (const y of [min.y, max.y]) {
        for (const z of [min.z, max.z]) corners.push(new THREE.Vector3(x, y, z));
      }
    }
    return corners;
  }

  private getOffsetRange(): [number, number] {
    const bounds = this.getBounds();
    let lo = Infinity, hi = -Infinity;
    for (const corner of this.getCorners(bounds)) {
      const d = corner.dot(this.plane.normal);
      lo = Math.min(lo, d);
      hi = Math.max(hi, d);
    }
    const pad = (hi - lo) * 0.05 + 0.01;
    return [lo - pad, hi + pad];
  }

  /** Small offset so the plane does not z-fight with the clicked face. */
  private epsilon(diag: number): number {
    return Math.max(diag * 1e-5, 1e-4);
  }

  private attach(): void {
    if (!this.gizmo.group.parent) this.scene.add(this.gizmo.group);
  }

  private detach(): void {
    if (this.gizmo.group.parent) this.scene.remove(this.gizmo.group);
  }
}
