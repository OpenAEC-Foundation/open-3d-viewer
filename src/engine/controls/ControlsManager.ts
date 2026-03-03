import * as THREE from 'three';
import { OrbitMode } from './OrbitMode';
import { WalkMode } from './WalkMode';
import type { CameraMode } from '../../state/slices/viewerSlice';

/**
 * Manages switching between Orbit and Walk controls
 * with smooth camera transitions.
 */
export class ControlsManager {
  private camera: THREE.PerspectiveCamera;

  orbit: OrbitMode;
  walk: WalkMode;

  private currentMode: CameraMode = 'orbit';
  private transitioning = false;

  constructor(camera: THREE.PerspectiveCamera, domElement: HTMLElement) {
    this.camera = camera;
    this.orbit = new OrbitMode(camera, domElement);
    this.walk = new WalkMode(camera, domElement);
    this.walk.setEnabled(false);
  }

  getMode(): CameraMode {
    return this.currentMode;
  }

  setMode(mode: CameraMode): void {
    if (mode === this.currentMode || this.transitioning) return;

    this.currentMode = mode;

    if (mode === 'walk') {
      this.orbit.setEnabled(false);
      this.walk.setEnabled(true);
    } else {
      this.walk.setEnabled(false);
      this.orbit.setEnabled(true);
      // Sync orbit target to current camera position
      const forward = new THREE.Vector3();
      this.camera.getWorldDirection(forward);
      const target = this.camera.position.clone().add(forward.multiplyScalar(5));
      this.orbit.setTarget(target);
    }
  }

  setCamera(camera: THREE.Camera): void {
    this.camera = camera as THREE.PerspectiveCamera;
    this.orbit.controls.object = camera;
    this.walk.setCamera(camera);
  }

  setWalkSpeed(speed: number): void {
    this.walk.speed = speed;
  }

  update(): void {
    if (this.currentMode === 'orbit') {
      this.orbit.update();
    } else {
      this.walk.update();
    }
  }

  dispose(): void {
    this.orbit.dispose();
    this.walk.dispose();
  }
}
