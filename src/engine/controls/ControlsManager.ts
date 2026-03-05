import * as THREE from 'three';
import { OrbitMode } from './OrbitMode';
import { WalkMode } from './WalkMode';
import type { CameraMode } from '../../state/slices/viewerSlice';

/**
 * Manages switching between Orbit and Walk controls
 * with smooth camera transitions.
 */
export class ControlsManager {
  orbit: OrbitMode;
  walk: WalkMode;

  private currentMode: CameraMode = 'orbit';
  private savedOrbitTarget = new THREE.Vector3();

  constructor(camera: THREE.PerspectiveCamera, domElement: HTMLElement) {
    this.orbit = new OrbitMode(camera, domElement);
    this.walk = new WalkMode(camera, domElement);
    this.walk.setEnabled(false);
  }

  getMode(): CameraMode {
    return this.currentMode;
  }

  setMode(mode: CameraMode): void {
    if (mode === this.currentMode) return;

    this.currentMode = mode;

    if (mode === 'walk') {
      // Save orbit target before switching
      this.savedOrbitTarget.copy(this.orbit.controls.target);
      this.orbit.setEnabled(false);
      this.walk.setEnabled(true);
    } else {
      this.walk.setEnabled(false);
      this.orbit.setEnabled(true);
      // Restore saved orbit target so zoom/pan speed stays correct
      this.orbit.setTarget(this.savedOrbitTarget);
    }
  }

  setCamera(camera: THREE.Camera): void {
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
