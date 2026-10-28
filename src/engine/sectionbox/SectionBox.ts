import * as THREE from 'three';
import { SectionBoxGizmo } from './SectionBoxGizmo';

/**
 * 6-plane clipping section box.
 * Manages clipping planes and an interactive gizmo with draggable face handles.
 */
export class SectionBox {
  /** 6 clipping planes: +X, -X, +Y, -Y, +Z, -Z */
  planes: THREE.Plane[] = [];
  /** Bounding box */
  min: THREE.Vector3;
  max: THREE.Vector3;
  /** Whether section box is enabled */
  enabled = false;
  /** Extra clipping planes combined with the box (e.g. the section plane) */
  extraPlanes: THREE.Plane[] = [];
  /** Helper objects (other gizmos) that must never be clipped */
  helpers: THREE.Object3D[] = [];

  /** Interactive gizmo with face handles */
  readonly gizmo = new SectionBoxGizmo();
  private scene: THREE.Scene;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
    this.min = new THREE.Vector3(-5, -5, -5);
    this.max = new THREE.Vector3(5, 5, 5);
    this.updatePlanes();
  }

  /**
   * Set bounds and update clipping planes.
   */
  setBounds(min: THREE.Vector3, max: THREE.Vector3): void {
    this.min.copy(min);
    this.max.copy(max);
    this.updatePlanes();
    this.updateVisual();
  }

  /**
   * Set bounds from scene bounding box with some padding.
   */
  fitToScene(sceneBounds: THREE.Box3, padding = 0.1): void {
    if (sceneBounds.isEmpty()) return;
    const size = sceneBounds.getSize(new THREE.Vector3());
    const pad = size.multiplyScalar(padding);
    this.min.copy(sceneBounds.min).sub(pad);
    this.max.copy(sceneBounds.max).add(pad);
    this.updatePlanes();
    this.updateVisual();
  }

  /**
   * Toggle section box on/off.
   */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (enabled) {
      this.updateVisual();
    } else {
      this.removeVisual();
    }
  }

  /**
   * All planes that currently clip the scene: the box planes (when enabled)
   * plus any extra planes such as a single section plane.
   */
  getActivePlanes(): THREE.Plane[] {
    return this.enabled ? [...this.planes, ...this.extraPlanes] : [...this.extraPlanes];
  }

  /**
   * Apply clipping planes to a material.
   */
  applyToMaterial(material: THREE.Material): void {
    const planes = this.getActivePlanes();
    material.clippingPlanes = planes;
    material.clipShadows = planes.length > 0;
  }

  /**
   * Apply clipping planes to all meshes in the scene.
   * Excludes gizmo/helper objects from clipping.
   */
  applyToScene(): void {
    const planes = this.getActivePlanes();
    const helpers = new Set<THREE.Object3D>([this.gizmo.group, ...this.helpers]);

    const visit = (child: THREE.Object3D) => {
      if (helpers.has(child)) return;
      if (child instanceof THREE.Mesh || child instanceof THREE.Points) {
        const materials = Array.isArray(child.material) ? child.material : [child.material];
        for (const mat of materials) {
          mat.clippingPlanes = planes;
          mat.clipShadows = planes.length > 0;
        }
      }
      for (const c of child.children) visit(c);
    };
    visit(this.scene);
  }

  /**
   * Get current bounds as a simple object.
   */
  getBounds(): { minX: number; minY: number; minZ: number; maxX: number; maxY: number; maxZ: number } {
    return {
      minX: this.min.x, minY: this.min.y, minZ: this.min.z,
      maxX: this.max.x, maxY: this.max.y, maxZ: this.max.z,
    };
  }

  dispose(): void {
    this.removeVisual();
    this.gizmo.dispose();
  }

  // ============================================================================
  // Private
  // ============================================================================

  private updatePlanes(): void {
    // Create 6 inward-facing planes
    this.planes = [
      new THREE.Plane(new THREE.Vector3(-1, 0, 0), this.max.x),   // +X face
      new THREE.Plane(new THREE.Vector3(1, 0, 0), -this.min.x),   // -X face
      new THREE.Plane(new THREE.Vector3(0, -1, 0), this.max.y),   // +Y face
      new THREE.Plane(new THREE.Vector3(0, 1, 0), -this.min.y),   // -Y face
      new THREE.Plane(new THREE.Vector3(0, 0, -1), this.max.z),   // +Z face
      new THREE.Plane(new THREE.Vector3(0, 0, 1), -this.min.z),   // -Z face
    ];
  }

  private updateVisual(): void {
    if (!this.enabled) {
      this.removeVisual();
      return;
    }

    // Update gizmo
    this.gizmo.update(this.min, this.max);
    if (!this.gizmo.group.parent) {
      this.scene.add(this.gizmo.group);
    }
  }

  private removeVisual(): void {
    if (this.gizmo.group.parent) {
      this.scene.remove(this.gizmo.group);
    }
    this.gizmo.dispose();
  }
}
