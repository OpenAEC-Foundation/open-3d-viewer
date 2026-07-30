import * as THREE from 'three';

export interface MeasurementVisual {
  id: string;
  group: THREE.Group;
  labelDiv: HTMLDivElement;
}

/**
 * Manages 3D measurement visuals: spheres at endpoints, line between them,
 * and an HTML overlay label showing the distance.
 */
export class MeasurementManager {
  private scene: THREE.Scene;
  private container: HTMLElement;
  private visuals = new Map<string, MeasurementVisual>();
  private tempGroup: THREE.Group | null = null;
  private tempLabel: HTMLDivElement | null = null;

  constructor(scene: THREE.Scene, container: HTMLElement) {
    this.scene = scene;
    this.container = container;
  }

  /** Set sphere size based on scene scale */
  private sphereRadius = 0.02;
  setSceneScale(sceneBounds: THREE.Box3) {
    if (sceneBounds.isEmpty()) return;
    const size = sceneBounds.getSize(new THREE.Vector3());
    this.sphereRadius = Math.max(size.x, size.y, size.z) * 0.004;
  }

  /** Show a temporary dot at the first measurement point */
  showTempPoint(point: THREE.Vector3) {
    this.clearTemp();
    const group = new THREE.Group();
    group.name = '__measurement_temp';
    const sphere = this.createSphere(point);
    group.add(sphere);
    this.scene.add(group);
    this.tempGroup = group;
  }

  /** Clear temp visuals */
  clearTemp() {
    if (this.tempGroup) {
      this.scene.remove(this.tempGroup);
      this.tempGroup = null;
    }
    if (this.tempLabel) {
      this.tempLabel.remove();
      this.tempLabel = null;
    }
  }

  /** Add a completed measurement */
  addMeasurement(id: string, p1: THREE.Vector3, p2: THREE.Vector3, label: string) {
    this.clearTemp();

    const group = new THREE.Group();
    group.name = `__measurement_${id}`;

    // Two endpoint spheres
    group.add(this.createSphere(p1));
    group.add(this.createSphere(p2));

    // Line between points
    const lineGeo = new THREE.BufferGeometry().setFromPoints([p1, p2]);
    const lineMat = new THREE.LineBasicMaterial({ color: 0xff3333, depthTest: false, linewidth: 2 });
    const line = new THREE.Line(lineGeo, lineMat);
    line.renderOrder = 999;
    group.add(line);

    this.scene.add(group);

    // HTML label at midpoint
    const labelDiv = document.createElement('div');
    labelDiv.className = 'measurement-label';
    labelDiv.textContent = label;
    labelDiv.dataset.measureId = id;
    Object.assign(labelDiv.style, {
      position: 'absolute',
      pointerEvents: 'none',
      background: 'rgba(20, 20, 20, 0.85)',
      color: '#ff5555',
      padding: '2px 6px',
      borderRadius: '3px',
      fontSize: '11px',
      fontWeight: '600',
      fontFamily: 'monospace',
      border: '1px solid rgba(255, 80, 80, 0.5)',
      zIndex: '100',
      whiteSpace: 'nowrap',
      transform: 'translate(-50%, -100%)',
    });
    this.container.appendChild(labelDiv);

    this.visuals.set(id, { id, group, labelDiv });
  }

  /** Update all label positions (call each frame or on camera move) */
  updateLabels(camera: THREE.Camera, width: number, height: number) {
    for (const visual of this.visuals.values()) {
      const children = visual.group.children;
      // Midpoint between the two spheres
      const s1 = children[0] as THREE.Mesh;
      const s2 = children[1] as THREE.Mesh;
      const mid = new THREE.Vector3().addVectors(s1.position, s2.position).multiplyScalar(0.5);

      const projected = mid.clone().project(camera);
      const x = (projected.x * 0.5 + 0.5) * width;
      const y = (-projected.y * 0.5 + 0.5) * height;

      // Hide if behind camera
      if (projected.z > 1) {
        visual.labelDiv.style.display = 'none';
      } else {
        visual.labelDiv.style.display = '';
        visual.labelDiv.style.left = `${x}px`;
        visual.labelDiv.style.top = `${y - 8}px`;
      }
    }
  }

  /** Update the text of a measurement label */
  updateLabel(id: string, text: string) {
    const visual = this.visuals.get(id);
    if (visual) visual.labelDiv.textContent = text;
  }

  /** Remove a specific measurement */
  remove(id: string) {
    const visual = this.visuals.get(id);
    if (!visual) return;
    this.scene.remove(visual.group);
    visual.labelDiv.remove();
    this.visuals.delete(id);
  }

  /** Clear all measurements */
  clear() {
    this.clearTemp();
    for (const visual of this.visuals.values()) {
      this.scene.remove(visual.group);
      visual.labelDiv.remove();
    }
    this.visuals.clear();
  }

  dispose() {
    this.clear();
  }

  private createSphere(pos: THREE.Vector3): THREE.Mesh {
    const geo = new THREE.SphereGeometry(this.sphereRadius, 12, 12);
    const mat = new THREE.MeshBasicMaterial({ color: 0xff3333, depthTest: false });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.position.copy(pos);
    mesh.renderOrder = 999;
    mesh.name = '__measurement_sphere';
    return mesh;
  }
}
