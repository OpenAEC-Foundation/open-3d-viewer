import * as THREE from 'three';
import { SectionPlane, snapNormal } from './SectionPlane';
import { worldPerPixel } from './SectionPlaneGizmo';

/**
 * Pointer interaction for the section plane.
 *
 * - Picking: the face under the mouse is previewed; a click places the plane
 *   on that face (any orientation, e.g. a sloped roof). Escape cancels.
 * - Placed: the arrow handle can be dragged to move the plane along its normal.
 *
 * Must be created before other canvas listeners (section box, selection),
 * because it stops the events it consumes from reaching them.
 */
export class SectionPlaneController {
  private sectionPlane: SectionPlane;
  private camera: THREE.Camera;
  private canvas: HTMLCanvasElement;
  private raycaster = new THREE.Raycaster();
  private mouse = new THREE.Vector2();

  private getPickTargets: () => THREE.Object3D[];
  private getClippingPlanes: () => THREE.Plane[];
  private onDragStart?: () => void;
  private onDragEnd?: () => void;
  private onPlace?: () => void;
  private onCancel?: () => void;

  /** Pending hover pick (throttled to one raycast per frame) */
  private hoverFrame = 0;
  private downPos = { x: 0, y: 0 };
  private dragging = false;
  private suppressClick = false;
  private drag = { startX: 0, startY: 0, startOffset: 0, dirX: 0, dirY: 0, pxPerUnit: 1 };

  constructor(
    sectionPlane: SectionPlane,
    camera: THREE.Camera,
    canvas: HTMLCanvasElement,
    options: {
      /** Objects that can be clicked to place a plane (the loaded models) */
      getPickTargets: () => THREE.Object3D[];
      /** Active clipping planes; hits in clipped-away geometry are ignored */
      getClippingPlanes: () => THREE.Plane[];
      onDragStart?: () => void;
      onDragEnd?: () => void;
      onPlace?: () => void;
      onCancel?: () => void;
    },
  ) {
    this.sectionPlane = sectionPlane;
    this.camera = camera;
    this.canvas = canvas;
    this.getPickTargets = options.getPickTargets;
    this.getClippingPlanes = options.getClippingPlanes;
    this.onDragStart = options.onDragStart;
    this.onDragEnd = options.onDragEnd;
    this.onPlace = options.onPlace;
    this.onCancel = options.onCancel;

    this.handlePointerMove = this.handlePointerMove.bind(this);
    this.handlePointerDown = this.handlePointerDown.bind(this);
    this.handlePointerUp = this.handlePointerUp.bind(this);
    this.handlePointerLeave = this.handlePointerLeave.bind(this);
    this.handleClick = this.handleClick.bind(this);
    this.handleKeyDown = this.handleKeyDown.bind(this);

    canvas.addEventListener('pointermove', this.handlePointerMove);
    canvas.addEventListener('pointerdown', this.handlePointerDown);
    canvas.addEventListener('pointerup', this.handlePointerUp);
    canvas.addEventListener('pointerleave', this.handlePointerLeave);
    canvas.addEventListener('click', this.handleClick);
    window.addEventListener('keydown', this.handleKeyDown);
  }

  setCamera(camera: THREE.Camera): void {
    this.camera = camera;
  }

  get isDragging(): boolean {
    return this.dragging;
  }

  dispose(): void {
    cancelAnimationFrame(this.hoverFrame);
    this.canvas.removeEventListener('pointermove', this.handlePointerMove);
    this.canvas.removeEventListener('pointerdown', this.handlePointerDown);
    this.canvas.removeEventListener('pointerup', this.handlePointerUp);
    this.canvas.removeEventListener('pointerleave', this.handlePointerLeave);
    this.canvas.removeEventListener('click', this.handleClick);
    window.removeEventListener('keydown', this.handleKeyDown);
  }

  // ============================================================================
  // Event handlers
  // ============================================================================

  private updateMouse(e: MouseEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    this.mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
    this.mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
  }

  private handlePointerMove(e: PointerEvent): void {
    const mode = this.sectionPlane.mode;
    if (mode === 'off') return;

    if (this.dragging) {
      const delta = (e.clientX - this.drag.startX) * this.drag.dirX + (e.clientY - this.drag.startY) * this.drag.dirY;
      this.sectionPlane.setOffset(this.drag.startOffset + delta / this.drag.pxPerUnit);
      return;
    }

    if (e.buttons !== 0) return;
    this.updateMouse(e);

    if (mode === 'picking') {
      this.canvas.style.cursor = 'crosshair';
      if (!this.hoverFrame) {
        this.hoverFrame = requestAnimationFrame(() => {
          this.hoverFrame = 0;
          if (this.sectionPlane.mode !== 'picking') return;
          const hit = this.pickFace();
          if (hit) this.sectionPlane.showPreview(hit.point, hit.normal);
          else this.sectionPlane.hidePreview();
        });
      }
      return;
    }

    // Placed: hover highlight on the arrow handle
    const overHandle = this.hitHandle();
    this.sectionPlane.gizmo.setHovered(overHandle);
    if (overHandle) {
      this.canvas.style.cursor = 'grab';
      // Keep the section box controller from resetting the cursor
      e.stopImmediatePropagation();
    } else {
      this.canvas.style.cursor = '';
    }
  }

  private handlePointerDown(e: PointerEvent): void {
    this.downPos = { x: e.clientX, y: e.clientY };
    if (this.sectionPlane.mode !== 'placed' || e.button !== 0) return;

    this.updateMouse(e);
    if (!this.hitHandle()) return;

    // Consume the event so the section box doesn't start a drag as well
    e.stopImmediatePropagation();
    this.startDrag(e);
  }

  private handlePointerUp(_e: PointerEvent): void {
    if (!this.dragging) return;
    this.dragging = false;
    this.suppressClick = true;
    this.sectionPlane.gizmo.setActive(false);
    this.canvas.style.cursor = '';
    this.onDragEnd?.();
  }

  private handlePointerLeave(): void {
    if (this.sectionPlane.mode === 'picking') this.sectionPlane.hidePreview();
  }

  private handleClick(e: MouseEvent): void {
    if (this.suppressClick) {
      // Click that ends a handle drag — don't let it select anything
      this.suppressClick = false;
      e.stopImmediatePropagation();
      return;
    }
    if (this.sectionPlane.mode !== 'picking') return;

    // While picking, clicks never select objects
    e.stopImmediatePropagation();
    const dx = e.clientX - this.downPos.x;
    const dy = e.clientY - this.downPos.y;
    if (Math.abs(dx) > 5 || Math.abs(dy) > 5) return; // orbit gesture

    this.updateMouse(e);
    const hit = this.pickFace();
    if (!hit) return;

    this.sectionPlane.place(hit.point, hit.normal);
    this.canvas.style.cursor = '';
    this.onPlace?.();
  }

  private handleKeyDown(e: KeyboardEvent): void {
    if (e.key !== 'Escape' || this.sectionPlane.mode !== 'picking') return;
    this.sectionPlane.hidePreview();
    this.canvas.style.cursor = '';
    this.onCancel?.();
  }

  // ============================================================================
  // Picking & dragging
  // ============================================================================

  /**
   * Raycast the models and return the first visible, unclipped face with its world normal
   * (facing the camera).
   */
  private pickFace(): { point: THREE.Vector3; normal: THREE.Vector3 } | null {
    this.raycaster.setFromCamera(this.mouse, this.camera);
    const hits = this.raycaster.intersectObjects(this.getPickTargets(), true);
    const clipping = this.getClippingPlanes();

    for (const hit of hits) {
      const obj = hit.object;
      if (!(obj instanceof THREE.Mesh) || !hit.face) continue;
      if (!this.isVisible(obj)) continue;
      if (clipping.some((p) => p.distanceToPoint(hit.point) < 0)) continue;

      const normalMatrix = new THREE.Matrix3().getNormalMatrix(obj.matrixWorld);
      const normal = hit.face.normal.clone().applyMatrix3(normalMatrix).normalize();
      if (normal.dot(this.raycaster.ray.direction) > 0) normal.negate();
      return { point: hit.point.clone(), normal: snapNormal(normal) };
    }
    return null;
  }

  private isVisible(obj: THREE.Object3D): boolean {
    for (let o: THREE.Object3D | null = obj; o; o = o.parent) {
      if (!o.visible) return false;
    }
    return true;
  }

  private hitHandle(): boolean {
    const meshes = this.sectionPlane.gizmo.getHandleMeshes();
    if (meshes.length === 0) return false;
    this.raycaster.setFromCamera(this.mouse, this.camera);
    return this.raycaster.intersectObjects(meshes, false).length > 0;
  }

  /**
   * Map mouse movement onto the plane normal. The normal is projected to screen
   * space; when looking straight along it, vertical mouse movement is used
   * (up = towards the camera).
   */
  private startDrag(e: PointerEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    const origin = this.sectionPlane.getHandlePosition();
    const normal = this.sectionPlane.plane.normal;
    const wpp = worldPerPixel(this.camera, origin, rect.height);
    const probe = wpp * 100;

    const toScreen = (p: THREE.Vector3) => {
      const ndc = p.clone().project(this.camera);
      return new THREE.Vector2((ndc.x + 1) / 2 * rect.width, (1 - ndc.y) / 2 * rect.height);
    };
    const axis = toScreen(origin.clone().addScaledVector(normal, probe)).sub(toScreen(origin));
    const len = axis.length();

    if (len > 30) {
      this.drag.dirX = axis.x / len;
      this.drag.dirY = axis.y / len;
      this.drag.pxPerUnit = len / probe;
    } else {
      const viewDir = new THREE.Vector3();
      this.camera.getWorldDirection(viewDir);
      const towardsCamera = normal.dot(viewDir) < 0;
      this.drag.dirX = 0;
      this.drag.dirY = towardsCamera ? -1 : 1;
      this.drag.pxPerUnit = 1 / wpp;
    }

    this.canvas.setPointerCapture(e.pointerId);
    this.drag.startX = e.clientX;
    this.drag.startY = e.clientY;
    this.drag.startOffset = this.sectionPlane.getOffset();
    this.dragging = true;
    this.sectionPlane.gizmo.setActive(true);
    this.canvas.style.cursor = 'grabbing';
    this.onDragStart?.();
  }
}
