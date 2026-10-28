import * as THREE from 'three';

/**
 * Visual representation of a single section plane.
 *
 * Shows a semi-transparent plate with an outline (same styling as the
 * section box faces) and an arrow handle that can be dragged to move the
 * plane along its normal. The arrow keeps a constant size on screen.
 */

const FACE_COLOR = 0x00ccff;
const FACE_OPACITY = 0.06;
const FACE_HOVER_OPACITY = 0.25;
const FACE_ACTIVE_OPACITY = 0.35;
const HOVER_COLOR = 0x44eeff;
const ACTIVE_COLOR = 0x00ff88;
const EDGE_OPACITY = 0.8;
/** Arrow handle length in screen pixels */
const HANDLE_PIXELS = 70;

/**
 * World units covered by one screen pixel at the given world position.
 */
export function worldPerPixel(camera: THREE.Camera, position: THREE.Vector3, viewportHeight: number): number {
  if (camera instanceof THREE.OrthographicCamera) {
    return (camera.top - camera.bottom) / camera.zoom / viewportHeight;
  }
  if (camera instanceof THREE.PerspectiveCamera) {
    const camPos = new THREE.Vector3().setFromMatrixPosition(camera.matrixWorld);
    const dist = camPos.distanceTo(position);
    return (2 * dist * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2))) / viewportHeight;
  }
  return 0.01;
}

export interface PlateOptions {
  /** Plate center */
  center: THREE.Vector3;
  /** In-plane axes and normal (right-handed basis) */
  u: THREE.Vector3;
  v: THREE.Vector3;
  normal: THREE.Vector3;
  width: number;
  height: number;
  /** Direction the arrow points (towards the side that is cut away) */
  arrowDir: THREE.Vector3;
  /** Preview style (hover while picking) instead of a placed plane */
  preview: boolean;
}

export class SectionPlaneGizmo {
  readonly group = new THREE.Group();
  private plate = new THREE.Group();
  private face: THREE.Mesh | null = null;
  private edge: THREE.LineLoop | null = null;
  private handle = new THREE.Group();
  private handleMaterial: THREE.MeshBasicMaterial;
  private handlePick: THREE.Mesh;
  private preview = false;
  private hovered = false;
  private active = false;

  constructor() {
    this.group.name = 'section-plane-gizmo';
    this.group.renderOrder = 999;
    this.group.add(this.plate);

    // Arrow along +Y with unit length; scaled to screen size every frame
    this.handleMaterial = new THREE.MeshBasicMaterial({
      color: FACE_COLOR,
      transparent: true,
      opacity: 0.9,
      depthTest: false,
      depthWrite: false,
    });
    const shaft = new THREE.Mesh(new THREE.CylinderGeometry(0.035, 0.035, 0.7, 12), this.handleMaterial);
    shaft.position.y = 0.35;
    const head = new THREE.Mesh(new THREE.ConeGeometry(0.11, 0.3, 20), this.handleMaterial);
    head.position.y = 0.85;
    const base = new THREE.Mesh(new THREE.SphereGeometry(0.07, 16, 12), this.handleMaterial);
    // Invisible, thicker cylinder so the arrow is easy to grab
    this.handlePick = new THREE.Mesh(
      new THREE.CylinderGeometry(0.16, 0.16, 1.1, 8),
      new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthTest: false, depthWrite: false }),
    );
    this.handlePick.position.y = 0.5;
    for (const m of [shaft, head, base, this.handlePick]) {
      m.renderOrder = 1001;
      this.handle.add(m);
    }
    this.handle.visible = false;
    this.group.add(this.handle);
  }

  /**
   * Rebuild the plate (and position the arrow) for the given plane.
   */
  update(opts: PlateOptions): void {
    this.clearPlate();
    this.preview = opts.preview;

    const basis = new THREE.Matrix4().makeBasis(opts.u, opts.v, opts.normal);
    this.plate.position.copy(opts.center);
    this.plate.quaternion.setFromRotationMatrix(basis);

    const mat = new THREE.MeshBasicMaterial({
      color: FACE_COLOR,
      transparent: true,
      opacity: FACE_OPACITY,
      side: THREE.DoubleSide,
      depthWrite: false,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      polygonOffsetUnits: -1,
    });
    this.face = new THREE.Mesh(new THREE.PlaneGeometry(opts.width, opts.height), mat);
    this.face.renderOrder = 999;
    this.plate.add(this.face);

    const hw = opts.width / 2;
    const hh = opts.height / 2;
    const edgeGeo = new THREE.BufferGeometry();
    edgeGeo.setAttribute('position', new THREE.Float32BufferAttribute([
      -hw, -hh, 0,  hw, -hh, 0,  hw, hh, 0,  -hw, hh, 0,
    ], 3));
    this.edge = new THREE.LineLoop(edgeGeo, new THREE.LineBasicMaterial({
      color: FACE_COLOR,
      transparent: true,
      opacity: EDGE_OPACITY,
      depthTest: false,
    }));
    this.edge.renderOrder = 1000;
    this.plate.add(this.edge);

    this.handle.position.copy(opts.center);
    this.handle.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), opts.arrowDir);
    this.handle.visible = true;

    this.updateAppearance();
  }

  /** Remove the plate and hide the arrow. */
  clear(): void {
    this.clearPlate();
    this.handle.visible = false;
  }

  /** Meshes that start a drag when clicked. */
  getHandleMeshes(): THREE.Mesh[] {
    return this.handle.visible && !this.preview ? [this.handlePick] : [];
  }

  getHandlePosition(): THREE.Vector3 {
    return this.handle.position.clone();
  }

  setHovered(hovered: boolean): void {
    if (this.hovered === hovered) return;
    this.hovered = hovered;
    this.updateAppearance();
  }

  setActive(active: boolean): void {
    this.active = active;
    this.updateAppearance();
  }

  /**
   * Keep the arrow at a constant size on screen. Call once per frame.
   */
  updateScreenScale(camera: THREE.Camera, viewportHeight: number): void {
    if (!this.handle.visible || viewportHeight === 0) return;
    const scale = HANDLE_PIXELS * worldPerPixel(camera, this.handle.position, viewportHeight);
    this.handle.scale.setScalar(scale);
  }

  dispose(): void {
    this.clearPlate();
    this.handle.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry.dispose();
        (child.material as THREE.Material).dispose();
      }
    });
  }

  // ============================================================================
  // Private
  // ============================================================================

  private clearPlate(): void {
    if (this.face) {
      this.face.geometry.dispose();
      (this.face.material as THREE.Material).dispose();
      this.plate.remove(this.face);
      this.face = null;
    }
    if (this.edge) {
      this.edge.geometry.dispose();
      (this.edge.material as THREE.Material).dispose();
      this.plate.remove(this.edge);
      this.edge = null;
    }
  }

  private updateAppearance(): void {
    let color = FACE_COLOR;
    let opacity = FACE_OPACITY;
    if (this.active) {
      color = ACTIVE_COLOR;
      opacity = FACE_ACTIVE_OPACITY;
    } else if (this.hovered || this.preview) {
      color = HOVER_COLOR;
      opacity = FACE_HOVER_OPACITY;
    }

    if (this.face) {
      const mat = this.face.material as THREE.MeshBasicMaterial;
      mat.color.setHex(color);
      mat.opacity = opacity;
    }
    if (this.edge) {
      (this.edge.material as THREE.LineBasicMaterial).color.setHex(color);
    }
    this.handleMaterial.color.setHex(color);
  }
}
