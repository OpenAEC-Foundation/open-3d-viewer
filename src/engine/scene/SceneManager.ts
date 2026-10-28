import * as THREE from 'three';
import type { LoaderResult, ThreeJSResult } from '../loaders/types';
import { isThreeJSResult } from '../loaders/types';
import { useAppStore } from '../../state/appStore';

/**
 * Manages the Three.js scene: adds/removes models, handles render modes.
 */
export class SceneManager {
  private scene: THREE.Scene;
  private modelGroups = new Map<string, THREE.Group>();
  /** Stored original materials when in hiddenline mode */
  private originalMaterials = new Map<THREE.Mesh, THREE.Material | THREE.Material[]>();
  /** Edge line objects added in hiddenline mode */
  private edgeLines: THREE.LineSegments[] = [];

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  /**
   * Add a loaded model to the scene.
   */
  addModel(id: string, result: LoaderResult): void {
    let group: THREE.Group;
    let totalVertices: number;
    let totalTriangles: number;
    let bounds: { min: THREE.Vector3; max: THREE.Vector3 };

    if (isThreeJSResult(result)) {
      const tjr = result as ThreeJSResult;
      group = tjr.scene instanceof THREE.Group ? tjr.scene : new THREE.Group();
      if (!(tjr.scene instanceof THREE.Group)) {
        group.add(...tjr.scene.children);
      }
      totalVertices = tjr.totalVertices;
      totalTriangles = tjr.totalTriangles;
      bounds = tjr.bounds;
    } else {
      // ParsedModel - build Three.js objects from parsed geometry
      group = new THREE.Group();
      totalVertices = result.totalVertices;
      totalTriangles = result.totalTriangles;
      bounds = result.bounds;

      for (const geom of result.geometries) {
        const bufferGeom = new THREE.BufferGeometry();
        bufferGeom.setAttribute('position', new THREE.BufferAttribute(geom.positions, 3));

        if (geom.indices) {
          bufferGeom.setIndex(new THREE.BufferAttribute(geom.indices, 1));
        }
        if (geom.normals) {
          bufferGeom.setAttribute('normal', new THREE.BufferAttribute(geom.normals, 3));
        } else {
          bufferGeom.computeVertexNormals();
        }
        if (geom.uvs) {
          bufferGeom.setAttribute('uv', new THREE.BufferAttribute(geom.uvs, 2));
        }
        if (geom.colors) {
          bufferGeom.setAttribute('color', new THREE.BufferAttribute(geom.colors, 3));
        }

        const mat = this.createMaterial(geom.material, !!geom.colors);
        const mesh = new THREE.Mesh(bufferGeom, mat);
        if (geom.name) mesh.name = geom.name;
        group.add(mesh);
      }
    }

    group.name = `model-${id}`;
    this.scene.add(group);
    this.modelGroups.set(id, group);

    // Register layers from child groups or individual meshes
    const layers: { name: string; visible: boolean; triangles: number }[] = [];
    for (const child of group.children) {
      if (child instanceof THREE.Group) {
        let tris = 0;
        child.traverse((c) => {
          if (c instanceof THREE.Mesh) {
            const geo = c.geometry;
            tris += geo.index ? geo.index.count / 3 : geo.attributes.position.count / 3;
          }
        });
        layers.push({ name: child.name, visible: true, triangles: Math.round(tris) });
      } else if (child instanceof THREE.Mesh) {
        const geo = child.geometry;
        const tris = geo.index ? geo.index.count / 3 : geo.attributes.position.count / 3;
        layers.push({ name: child.name, visible: true, triangles: Math.round(tris) });
      }
    }
    if (layers.length > 0) {
      useAppStore.getState().setLayers(layers);
    }

    // Update store
    useAppStore.getState().updateModelStats(id, totalVertices, totalTriangles, {
      minX: bounds.min.x,
      minY: bounds.min.y,
      minZ: bounds.min.z,
      maxX: bounds.max.x,
      maxY: bounds.max.y,
      maxZ: bounds.max.z,
    });
    useAppStore.getState().updateModelProgress(id, 1, 'Ready');
    useAppStore.getState().removeLoadingFile(id);
  }

  /**
   * Remove a model from the scene.
   */
  removeModel(id: string): void {
    const group = this.modelGroups.get(id);
    if (!group) return;

    this.scene.remove(group);
    group.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry.dispose();
        if (Array.isArray(child.material)) {
          child.material.forEach((m) => m.dispose());
        } else {
          child.material.dispose();
        }
      }
    });
    this.modelGroups.delete(id);
  }

  /**
   * Root groups of all loaded models.
   */
  getModelGroups(): THREE.Group[] {
    return [...this.modelGroups.values()];
  }

  /**
   * Get bounding box of all models.
   */
  getSceneBounds(): THREE.Box3 {
    const box = new THREE.Box3();
    for (const group of this.modelGroups.values()) {
      box.expandByObject(group);
    }
    return box;
  }

  /**
   * Toggle visibility of a named layer (child group or mesh) within models.
   */
  setLayerVisible(name: string, visible: boolean): void {
    for (const group of this.modelGroups.values()) {
      for (const child of group.children) {
        if (child.name === name) {
          child.visible = visible;
        }
      }
    }
  }

  /**
   * Apply render mode to all models.
   */
  setRenderMode(mode: 'solid' | 'wireframe' | 'points' | 'hiddenline'): void {
    // Clean up previous hiddenline state
    this.clearHiddenLine();

    if (mode === 'hiddenline') {
      this.applyHiddenLine();
      return;
    }

    for (const group of this.modelGroups.values()) {
      group.traverse((child) => {
        if (child instanceof THREE.Mesh) {
          const mat = child.material as THREE.Material;
          if ('wireframe' in mat) {
            (mat as THREE.MeshStandardMaterial).wireframe = mode === 'wireframe';
          }
        }
      });
    }
  }

  /**
   * Apply hidden line mode: white faces for occlusion + dark edge lines.
   */
  private applyHiddenLine(): void {
    const whiteMat = new THREE.MeshBasicMaterial({
      color: 0xf8f8f8,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });

    const lineMat = new THREE.LineBasicMaterial({ color: 0x222222, linewidth: 1 });

    for (const group of this.modelGroups.values()) {
      group.traverse((child) => {
        if (child instanceof THREE.Mesh) {
          // Store original material
          this.originalMaterials.set(child, child.material);
          child.material = whiteMat;

          // Create edge geometry (threshold ~15 degrees for crisp edges)
          const edges = new THREE.EdgesGeometry(child.geometry, 15);
          const lineSegments = new THREE.LineSegments(edges, lineMat);
          lineSegments.name = '__hiddenline_edge';
          lineSegments.renderOrder = 1;
          child.add(lineSegments);
          this.edgeLines.push(lineSegments);
        }
      });
    }
  }

  /**
   * Remove hidden line mode: restore original materials and remove edge lines.
   */
  private clearHiddenLine(): void {
    // Restore original materials
    for (const [mesh, mat] of this.originalMaterials) {
      mesh.material = mat;
    }
    this.originalMaterials.clear();

    // Remove edge lines
    for (const line of this.edgeLines) {
      line.parent?.remove(line);
      line.geometry.dispose();
    }
    this.edgeLines = [];
  }

  private createMaterial(
    parsed: import('../loaders/types').ParsedMaterial | undefined,
    hasVertexColors: boolean
  ): THREE.MeshStandardMaterial {
    const mat = new THREE.MeshStandardMaterial({
      color: parsed?.color ? new THREE.Color(parsed.color[0], parsed.color[1], parsed.color[2]) : 0x888888,
      metalness: parsed?.metalness ?? 0.1,
      roughness: parsed?.roughness ?? 0.7,
      opacity: parsed?.opacity ?? 1,
      transparent: (parsed?.opacity ?? 1) < 1,
      side: parsed?.doubleSided ? THREE.DoubleSide : THREE.FrontSide,
      vertexColors: hasVertexColors,
    });
    return mat;
  }
}
