import * as THREE from 'three';

/**
 * SectionCapRenderer — Renders solid black caps on section box cut planes.
 *
 * Uses stencil buffer technique (per Three.js clipping_stencil example):
 * Per active clipping plane P:
 * 1. Stencil pass back-faces:  clip by P only, depthTest OFF, increment stencil
 * 2. Stencil pass front-faces: clip by P only, depthTest OFF, decrement stencil
 * 3. Cap fill: render plane at P, clip by OTHER 5 planes, stencil ≠ 0
 *
 * This correctly detects "inside solid geometry" even with the existing depth buffer.
 */
export class SectionCapRenderer {
  enabled = true;

  private renderer: THREE.WebGLRenderer;
  private capScene = new THREE.Scene();
  private capMeshes: THREE.Mesh[] = [];
  private capGeometry: THREE.PlaneGeometry;

  // Stencil materials — depthTest OFF, clip by current plane only
  private stencilBackMat: THREE.MeshBasicMaterial;
  private stencilFrontMat: THREE.MeshBasicMaterial;

  // Cap fill material — black, stencil ≠ 0, clip by other 5 planes
  private capMat: THREE.MeshBasicMaterial;

  // Temp vectors
  private _box = new THREE.Box3();
  private _size = new THREE.Vector3();

  constructor(renderer: THREE.WebGLRenderer) {
    this.renderer = renderer;

    // Back-face stencil: increment on all back faces (no depth test)
    this.stencilBackMat = new THREE.MeshBasicMaterial({
      side: THREE.BackSide,
      colorWrite: false,
      depthWrite: false,
      depthTest: false,
      stencilWrite: true,
      stencilFunc: THREE.AlwaysStencilFunc,
      stencilFail: THREE.IncrementWrapStencilOp,
      stencilZFail: THREE.IncrementWrapStencilOp,
      stencilZPass: THREE.IncrementWrapStencilOp,
    });

    // Front-face stencil: decrement on all front faces (no depth test)
    this.stencilFrontMat = new THREE.MeshBasicMaterial({
      side: THREE.FrontSide,
      colorWrite: false,
      depthWrite: false,
      depthTest: false,
      stencilWrite: true,
      stencilFunc: THREE.AlwaysStencilFunc,
      stencilFail: THREE.DecrementWrapStencilOp,
      stencilZFail: THREE.DecrementWrapStencilOp,
      stencilZPass: THREE.DecrementWrapStencilOp,
    });

    // Cap fill material: black where stencil ≠ 0, reset stencil to 0 after draw
    this.capMat = new THREE.MeshBasicMaterial({
      color: 0x000000,
      side: THREE.DoubleSide,
      depthWrite: true,
      depthTest: true,
      stencilWrite: true,
      stencilFunc: THREE.NotEqualStencilFunc,
      stencilRef: 0,
      stencilFail: THREE.ReplaceStencilOp,
      stencilZFail: THREE.ReplaceStencilOp,
      stencilZPass: THREE.ReplaceStencilOp,
    });

    // Plane geometry for caps
    this.capGeometry = new THREE.PlaneGeometry(1, 1);

    // Pre-create 6 cap meshes
    for (let i = 0; i < 6; i++) {
      const mesh = new THREE.Mesh(this.capGeometry, this.capMat.clone());
      mesh.name = '__section_cap';
      mesh.frustumCulled = false;
      mesh.renderOrder = 1000;
      this.capMeshes.push(mesh);
    }
  }

  /**
   * Multi-pass render for section caps. Call after the main render pass.
   */
  render(scene: THREE.Scene, camera: THREE.Camera, planes: THREE.Plane[]): void {
    if (!this.enabled || planes.length === 0) return;

    // Get model bounds to check which planes actually intersect
    const bounds = this.getModelBounds(scene);
    if (bounds.isEmpty()) return;
    bounds.getSize(this._size);

    const activePlanes: { plane: THREE.Plane; index: number }[] = [];
    for (let i = 0; i < planes.length; i++) {
      if (this.planeIntersectsBox(planes[i], bounds)) {
        activePlanes.push({ plane: planes[i], index: i });
      }
    }
    if (activePlanes.length === 0) return;

    // Hide non-model objects during stencil passes
    const hidden = this.hideNonModelObjects(scene);

    // Save renderer state
    const autoClear = this.renderer.autoClear;
    this.renderer.autoClear = false;

    for (const { plane, index } of activePlanes) {
      // Position cap mesh on the clipping plane
      this.positionCapPlane(plane, index);

      // Clear stencil buffer
      this.renderer.clearStencil();

      // Stencil passes: clip by ONLY this plane (not all 6)
      // This is the key insight from the Three.js clipping_stencil example:
      // clipping by one plane removes geometry on one side, creating a stencil
      // imbalance at the cross-section area
      this.stencilBackMat.clippingPlanes = [plane];
      this.stencilFrontMat.clippingPlanes = [plane];

      // Pass 1: Back faces → stencil increment
      scene.overrideMaterial = this.stencilBackMat;
      this.renderer.render(scene, camera);

      // Pass 2: Front faces → stencil decrement
      scene.overrideMaterial = this.stencilFrontMat;
      this.renderer.render(scene, camera);

      // Restore scene
      scene.overrideMaterial = null;

      // Pass 3: Render ONLY cap mesh (in separate scene) where stencil ≠ 0
      // Cap is clipped by all OTHER planes to stay within section box
      const otherPlanes = planes.filter((_, j) => j !== index);
      const capMesh = this.capMeshes[index];
      const mat = capMesh.material as THREE.MeshBasicMaterial;
      mat.clippingPlanes = otherPlanes;

      this.capScene.children.length = 0;
      this.capScene.add(capMesh);
      this.renderer.render(this.capScene, camera);
      this.capScene.remove(capMesh);
    }

    // Restore
    scene.overrideMaterial = null;
    this.renderer.autoClear = autoClear;
    this.restoreHiddenObjects(hidden);
  }

  /**
   * Position a cap mesh on the clipping plane, sized to cover the model bounds.
   */
  private positionCapPlane(plane: THREE.Plane, index: number): void {
    const mesh = this.capMeshes[index];
    const maxDim = Math.max(this._size.x, this._size.y, this._size.z) * 2;

    mesh.scale.set(maxDim, maxDim, 1);

    // Position at the plane's closest point to origin
    const normal = plane.normal;
    const pointOnPlane = new THREE.Vector3().copy(normal).multiplyScalar(-plane.constant);

    mesh.position.copy(pointOnPlane);

    // Orient the plane mesh to face along the clipping plane normal
    mesh.lookAt(
      pointOnPlane.x + normal.x,
      pointOnPlane.y + normal.y,
      pointOnPlane.z + normal.z,
    );
    mesh.updateMatrixWorld(true);
  }

  /**
   * Check if a plane intersects a bounding box.
   */
  private planeIntersectsBox(plane: THREE.Plane, box: THREE.Box3): boolean {
    const min = box.min;
    const max = box.max;
    const n = plane.normal;

    const pX = n.x >= 0 ? max.x : min.x;
    const pY = n.y >= 0 ? max.y : min.y;
    const pZ = n.z >= 0 ? max.z : min.z;

    const nX = n.x >= 0 ? min.x : max.x;
    const nY = n.y >= 0 ? min.y : max.y;
    const nZ = n.z >= 0 ? min.z : max.z;

    const distP = n.x * pX + n.y * pY + n.z * pZ + plane.constant;
    const distN = n.x * nX + n.y * nY + n.z * nZ + plane.constant;

    return distP >= 0 && distN <= 0;
  }

  /**
   * Get combined bounding box of all model-* children in the scene.
   */
  private getModelBounds(scene: THREE.Scene): THREE.Box3 {
    this._box.makeEmpty();
    for (const child of scene.children) {
      if (child.name.startsWith('model-') && child.visible) {
        this._box.expandByObject(child);
      }
    }
    return this._box;
  }

  /**
   * Hide non-model objects during stencil passes.
   */
  private hideNonModelObjects(scene: THREE.Scene): { obj: THREE.Object3D; wasVisible: boolean }[] {
    const hidden: { obj: THREE.Object3D; wasVisible: boolean }[] = [];
    for (const child of scene.children) {
      if (child.name.startsWith('model-')) continue;
      if (child.visible) {
        hidden.push({ obj: child, wasVisible: true });
        child.visible = false;
      }
    }
    return hidden;
  }

  private restoreHiddenObjects(hidden: { obj: THREE.Object3D; wasVisible: boolean }[]): void {
    for (const { obj, wasVisible } of hidden) {
      obj.visible = wasVisible;
    }
  }

  dispose(): void {
    this.capGeometry.dispose();
    this.stencilBackMat.dispose();
    this.stencilFrontMat.dispose();
    this.capMat.dispose();
    for (const mesh of this.capMeshes) {
      (mesh.material as THREE.Material).dispose();
    }
  }
}
