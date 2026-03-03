import * as THREE from 'three';

/**
 * Visual representation of the section box with 6 draggable face handles.
 *
 * Each face is a semi-transparent plane that highlights on hover.
 * The edge wireframe shows the box outline.
 */

export type FaceId = '+x' | '-x' | '+y' | '-y' | '+z' | '-z';

const FACE_COLOR = 0x00ccff;
const FACE_OPACITY = 0.06;
const FACE_HOVER_OPACITY = 0.25;
const FACE_ACTIVE_OPACITY = 0.35;
const EDGE_COLOR = 0x00ccff;
const EDGE_OPACITY = 0.8;

interface FaceHandle {
  id: FaceId;
  mesh: THREE.Mesh;
  normal: THREE.Vector3;
}

export class SectionBoxGizmo {
  readonly group = new THREE.Group();
  private faces: FaceHandle[] = [];
  private edgeLines: THREE.LineSegments | null = null;
  private hoveredFace: FaceId | null = null;
  private activeFace: FaceId | null = null;

  constructor() {
    this.group.name = 'section-box-gizmo';
    this.group.renderOrder = 999;
  }

  /**
   * Build or rebuild the gizmo geometry from current bounds.
   */
  update(min: THREE.Vector3, max: THREE.Vector3): void {
    this.dispose();

    const cx = (min.x + max.x) / 2;
    const cy = (min.y + max.y) / 2;
    const cz = (min.z + max.z) / 2;
    const sx = max.x - min.x;
    const sy = max.y - min.y;
    const sz = max.z - min.z;

    // Create 6 face handles
    const faceDefs: { id: FaceId; pos: [number, number, number]; rot: [number, number, number]; size: [number, number]; normal: [number, number, number] }[] = [
      { id: '+x', pos: [max.x, cy, cz], rot: [0, Math.PI / 2, 0],  size: [sz, sy], normal: [1, 0, 0] },
      { id: '-x', pos: [min.x, cy, cz], rot: [0, -Math.PI / 2, 0], size: [sz, sy], normal: [-1, 0, 0] },
      { id: '+y', pos: [cx, max.y, cz], rot: [-Math.PI / 2, 0, 0], size: [sx, sz], normal: [0, 1, 0] },
      { id: '-y', pos: [cx, min.y, cz], rot: [Math.PI / 2, 0, 0],  size: [sx, sz], normal: [0, -1, 0] },
      { id: '+z', pos: [cx, cy, max.z], rot: [0, 0, 0],            size: [sx, sy], normal: [0, 0, 1] },
      { id: '-z', pos: [cx, cy, min.z], rot: [0, Math.PI, 0],      size: [sx, sy], normal: [0, 0, -1] },
    ];

    for (const def of faceDefs) {
      const geo = new THREE.PlaneGeometry(def.size[0], def.size[1]);
      const mat = new THREE.MeshBasicMaterial({
        color: FACE_COLOR,
        transparent: true,
        opacity: FACE_OPACITY,
        side: THREE.DoubleSide,
        depthWrite: false,
      });

      const mesh = new THREE.Mesh(geo, mat);
      mesh.position.set(...def.pos);
      mesh.rotation.set(...def.rot);
      mesh.userData.faceId = def.id;
      mesh.renderOrder = 999;

      this.faces.push({
        id: def.id,
        mesh,
        normal: new THREE.Vector3(...def.normal),
      });
      this.group.add(mesh);
    }

    // Edge wireframe
    this.edgeLines = this.createEdgeLines(min, max);
    this.group.add(this.edgeLines);
  }

  /**
   * Get all face meshes for raycasting.
   */
  getFaceMeshes(): THREE.Mesh[] {
    return this.faces.map((f) => f.mesh);
  }

  /**
   * Get the face info for a given mesh (from raycast hit).
   */
  getFaceByMesh(mesh: THREE.Mesh): FaceHandle | undefined {
    return this.faces.find((f) => f.mesh === mesh);
  }

  /**
   * Get face info by ID.
   */
  getFaceById(id: FaceId): FaceHandle | undefined {
    return this.faces.find((f) => f.id === id);
  }

  /**
   * Set hovered face (highlight effect).
   */
  setHoveredFace(id: FaceId | null): void {
    if (this.hoveredFace === id) return;
    this.hoveredFace = id;
    this.updateFaceAppearance();
  }

  /**
   * Set active/dragged face.
   */
  setActiveFace(id: FaceId | null): void {
    this.activeFace = id;
    this.updateFaceAppearance();
  }

  dispose(): void {
    for (const face of this.faces) {
      face.mesh.geometry.dispose();
      (face.mesh.material as THREE.Material).dispose();
      this.group.remove(face.mesh);
    }
    this.faces = [];

    if (this.edgeLines) {
      this.edgeLines.geometry.dispose();
      (this.edgeLines.material as THREE.Material).dispose();
      this.group.remove(this.edgeLines);
      this.edgeLines = null;
    }
  }

  // ============================================================================
  // Private
  // ============================================================================

  private updateFaceAppearance(): void {
    for (const face of this.faces) {
      const mat = face.mesh.material as THREE.MeshBasicMaterial;
      if (face.id === this.activeFace) {
        mat.opacity = FACE_ACTIVE_OPACITY;
        mat.color.setHex(0x00ff88);
      } else if (face.id === this.hoveredFace) {
        mat.opacity = FACE_HOVER_OPACITY;
        mat.color.setHex(0x44eeff);
      } else {
        mat.opacity = FACE_OPACITY;
        mat.color.setHex(FACE_COLOR);
      }
    }
  }

  private createEdgeLines(min: THREE.Vector3, max: THREE.Vector3): THREE.LineSegments {
    const verts = [
      // Bottom face
      min.x, min.y, min.z,  max.x, min.y, min.z,
      max.x, min.y, min.z,  max.x, min.y, max.z,
      max.x, min.y, max.z,  min.x, min.y, max.z,
      min.x, min.y, max.z,  min.x, min.y, min.z,
      // Top face
      min.x, max.y, min.z,  max.x, max.y, min.z,
      max.x, max.y, min.z,  max.x, max.y, max.z,
      max.x, max.y, max.z,  min.x, max.y, max.z,
      min.x, max.y, max.z,  min.x, max.y, min.z,
      // Vertical edges
      min.x, min.y, min.z,  min.x, max.y, min.z,
      max.x, min.y, min.z,  max.x, max.y, min.z,
      max.x, min.y, max.z,  max.x, max.y, max.z,
      min.x, min.y, max.z,  min.x, max.y, max.z,
    ];

    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(verts, 3));
    const mat = new THREE.LineBasicMaterial({
      color: EDGE_COLOR,
      transparent: true,
      opacity: EDGE_OPACITY,
      depthTest: false,
    });

    const lines = new THREE.LineSegments(geo, mat);
    lines.renderOrder = 1000;
    return lines;
  }
}
