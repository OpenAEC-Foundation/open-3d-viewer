import * as THREE from 'three';
import { SectionBoxGizmo } from './SectionBoxGizmo';
import { CAP_LAYER } from './SectionCaps';

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
   * Apply clipping planes to a material.
   */
  applyToMaterial(material: THREE.Material): void {
    if (this.enabled) {
      material.clippingPlanes = this.planes;
      material.clipShadows = true;
    } else {
      material.clippingPlanes = [];
    }
  }

  /**
   * Apply clipping planes to all meshes in the scene.
   * Excludes the gizmo's own meshes from clipping.
   */
  applyToScene(): void {
    this.scene.traverse((child) => {
      // Skip gizmo's own objects
      if (child === this.gizmo.group || this.gizmo.group.getObjectById(child.id)) return;
      // the ground grid is not cut and has no caps
      if (child.userData.isGrid) return;
      // model meshes take part in the cap stencil passes (SectionCaps)
      if (child instanceof THREE.Mesh && !child.userData.isHighlight) child.layers.enable(CAP_LAYER);

      if (child instanceof THREE.Mesh || child instanceof THREE.Points) {
        const materials = Array.isArray(child.material) ? child.material : [child.material];
        for (const mat of materials) {
          if (this.enabled) {
            mat.clippingPlanes = this.planes;
            mat.clipShadows = true;
          } else {
            mat.clippingPlanes = [];
          }
        }
      }
    });
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
