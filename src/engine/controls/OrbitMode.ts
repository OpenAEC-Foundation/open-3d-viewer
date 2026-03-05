import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

export class OrbitMode {
  controls: OrbitControls;

  constructor(camera: THREE.Camera, domElement: HTMLElement) {
    this.controls = new OrbitControls(camera, domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.1;
    this.controls.screenSpacePanning = true;
    this.controls.minDistance = 0.1;
    this.controls.maxDistance = 10000;
    this.controls.enablePan = true;
    this.controls.panSpeed = 1;
    this.controls.rotateSpeed = 0.8;
    this.controls.zoomSpeed = 1.2;
    // Z-up coordinate system
    camera.up.set(0, 0, 1);
    this.controls.object.up.set(0, 0, 1);
  }

  update() {
    this.controls.update();
  }

  setEnabled(enabled: boolean) {
    this.controls.enabled = enabled;
  }

  setTarget(target: THREE.Vector3) {
    this.controls.target.copy(target);
    this.controls.update();
  }

  dispose() {
    this.controls.dispose();
  }
}
