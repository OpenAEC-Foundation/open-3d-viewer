import * as THREE from 'three';
import { SectionBox } from './SectionBox';
import { SectionBoxGizmo, type FaceId } from './SectionBoxGizmo';

/**
 * Controls interactive dragging of section box face handles.
 *
 * Listens for pointer events on the canvas, raycasts against
 * the gizmo face meshes, and drags the hit face along its normal
 * to resize the section box in real-time.
 */
export class SectionBoxController {
  private sectionBox: SectionBox;
  private gizmo: SectionBoxGizmo;
  private camera: THREE.Camera;
  private canvas: HTMLCanvasElement;
  private raycaster = new THREE.Raycaster();
  private mouse = new THREE.Vector2();

  /** Currently dragged face */
  private dragFace: FaceId | null = null;
  /** The face's axis (positive x, y or z) and the point on it where the drag started */
  private dragAxis = new THREE.Vector3();
  private dragOrigin = new THREE.Vector3();
  /** The face coordinate when the drag started */
  private dragStartCoord = 0;
  /** Callback to temporarily disable orbit controls during drag */
  private onDragStart?: () => void;
  private onDragEnd?: () => void;
  /** Called when bounds change so clipping planes update */
  private onBoundsChange?: () => void;

  setCamera(camera: THREE.Camera): void {
    this.camera = camera;
  }

  /** Minimum box size (meters) to prevent collapse */
  private static MIN_SIZE = 0.05;

  constructor(
    sectionBox: SectionBox,
    gizmo: SectionBoxGizmo,
    camera: THREE.Camera,
    canvas: HTMLCanvasElement,
    callbacks?: {
      onDragStart?: () => void;
      onDragEnd?: () => void;
      onBoundsChange?: () => void;
    },
  ) {
    this.sectionBox = sectionBox;
    this.gizmo = gizmo;
    this.camera = camera;
    this.canvas = canvas;
    this.onDragStart = callbacks?.onDragStart;
    this.onDragEnd = callbacks?.onDragEnd;
    this.onBoundsChange = callbacks?.onBoundsChange;

    this.handlePointerMove = this.handlePointerMove.bind(this);
    this.handlePointerDown = this.handlePointerDown.bind(this);
    this.handlePointerUp = this.handlePointerUp.bind(this);

    canvas.addEventListener('pointermove', this.handlePointerMove);
    canvas.addEventListener('pointerdown', this.handlePointerDown);
    canvas.addEventListener('pointerup', this.handlePointerUp);
  }

  /** Whether a drag is currently active (used to suppress orbit). */
  get isDragging(): boolean {
    return this.dragFace !== null;
  }

  dispose(): void {
    this.canvas.removeEventListener('pointermove', this.handlePointerMove);
    this.canvas.removeEventListener('pointerdown', this.handlePointerDown);
    this.canvas.removeEventListener('pointerup', this.handlePointerUp);
  }

  // ============================================================================
  // Event handlers
  // ============================================================================

  private updateMouse(e: PointerEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    this.mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
    this.mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
  }

  private handlePointerMove(e: PointerEvent): void {
    if (!this.sectionBox.enabled) return;

    this.updateMouse(e);

    if (this.dragFace) {
      // Dragging: the face follows the mouse along its own axis
      this.raycaster.setFromCamera(this.mouse, this.camera);
      const t = this.axisParameter(this.raycaster.ray);
      if (t !== null) {
        this.applyDrag(this.dragFace, this.dragStartCoord + t);
      }
      return;
    }

    // Hover detection
    this.raycaster.setFromCamera(this.mouse, this.camera);
    const hits = this.raycaster.intersectObjects(this.gizmo.getFaceMeshes(), false);
    if (hits.length > 0) {
      const face = this.gizmo.getFaceByMesh(hits[0].object as THREE.Mesh);
      this.gizmo.setHoveredFace(face?.id ?? null);
      this.canvas.style.cursor = 'grab';
    } else {
      this.gizmo.setHoveredFace(null);
      this.canvas.style.cursor = '';
    }
  }

  private handlePointerDown(e: PointerEvent): void {
    if (!this.sectionBox.enabled || e.button !== 0) return;

    this.updateMouse(e);
    this.raycaster.setFromCamera(this.mouse, this.camera);
    const hits = this.raycaster.intersectObjects(this.gizmo.getFaceMeshes(), false);
    if (hits.length === 0) return;

    const face = this.gizmo.getFaceByMesh(hits[0].object as THREE.Mesh);
    if (!face) return;

    e.stopPropagation();
    this.dragFace = face.id;
    this.gizmo.setActiveFace(face.id);
    this.canvas.style.cursor = 'grabbing';

    // Drag along the face's axis (always the positive x, y or z direction, so the minus faces
    // move with the mouse too; projecting on their outward normal reversed them), from the hit
    // point: the face goes to the point of that axis nearest to the mouse ray.
    this.dragAxis.set(Math.abs(face.normal.x), Math.abs(face.normal.y), Math.abs(face.normal.z));
    this.dragOrigin.copy(hits[0].point);
    this.dragStartCoord = this.getFacePosition(face.id);

    this.onDragStart?.();
  }

  private handlePointerUp(_e: PointerEvent): void {
    if (this.dragFace) {
      this.dragFace = null;
      this.gizmo.setActiveFace(null);
      this.canvas.style.cursor = '';
      this.onDragEnd?.();
    }
  }

  // ============================================================================
  // Drag logic
  // ============================================================================

  /**
   * Distance along the drag axis (from where the drag started) of the axis point nearest to the
   * ray; null when the axis points straight at the viewer (no usable direction).
   */
  private axisParameter(ray: THREE.Ray): number | null {
    const u = this.dragAxis;
    const v = ray.direction;
    const w = new THREE.Vector3().subVectors(this.dragOrigin, ray.origin);
    const b = u.dot(v);
    const denom = 1 - b * b; // |u| = |v| = 1
    if (denom < 1e-6) return null;
    return (b * v.dot(w) - u.dot(w)) / denom;
  }

  private applyDrag(faceId: FaceId, newPos: number): void {
    const face = this.gizmo.getFaceById(faceId);
    if (!face) return;

    // Clamp to prevent box collapse
    const min = this.sectionBox.min.clone();
    const max = this.sectionBox.max.clone();

    switch (faceId) {
      case '+x': max.x = Math.max(min.x + SectionBoxController.MIN_SIZE, newPos); break;
      case '-x': min.x = Math.min(max.x - SectionBoxController.MIN_SIZE, newPos); break;
      case '+y': max.y = Math.max(min.y + SectionBoxController.MIN_SIZE, newPos); break;
      case '-y': min.y = Math.min(max.y - SectionBoxController.MIN_SIZE, newPos); break;
      case '+z': max.z = Math.max(min.z + SectionBoxController.MIN_SIZE, newPos); break;
      case '-z': min.z = Math.min(max.z - SectionBoxController.MIN_SIZE, newPos); break;
    }

    // Update section box
    this.sectionBox.setBounds(min, max);

    // Update gizmo visuals
    this.gizmo.update(min, max);
    this.gizmo.setActiveFace(faceId);

    // Re-apply clipping to scene
    this.sectionBox.applyToScene();
    this.onBoundsChange?.();
  }

  private getFacePosition(faceId: FaceId): number {
    switch (faceId) {
      case '+x': return this.sectionBox.max.x;
      case '-x': return this.sectionBox.min.x;
      case '+y': return this.sectionBox.max.y;
      case '-y': return this.sectionBox.min.y;
      case '+z': return this.sectionBox.max.z;
      case '-z': return this.sectionBox.min.z;
    }
  }
}
