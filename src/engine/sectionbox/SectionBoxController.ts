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
  /** Plane used for drag projection */
  private dragPlane = new THREE.Plane();
  /** Offset from face center to drag start (to prevent jumping) */
  private dragOffset = 0;
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
      // Dragging — project mouse onto drag plane
      this.raycaster.setFromCamera(this.mouse, this.camera);
      const intersection = new THREE.Vector3();
      if (this.raycaster.ray.intersectPlane(this.dragPlane, intersection)) {
        this.applyDrag(this.dragFace, intersection);
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

    // Build a plane perpendicular to the camera's view direction, passing through the hit point.
    // This gives the best dragging experience regardless of view angle.
    const hitPoint = hits[0].point;
    const cameraDir = new THREE.Vector3();
    this.camera.getWorldDirection(cameraDir);
    this.dragPlane.setFromNormalAndCoplanarPoint(cameraDir, hitPoint);

    // Store the initial face position along its normal to compute offset
    const facePos = this.getFacePosition(face.id);
    const projectedHit = hitPoint.dot(face.normal);
    this.dragOffset = facePos - projectedHit;

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

  private applyDrag(faceId: FaceId, worldPoint: THREE.Vector3): void {
    const face = this.gizmo.getFaceById(faceId);
    if (!face) return;

    // Project world point onto the face normal axis
    const newPos = worldPoint.dot(face.normal) + this.dragOffset;

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
