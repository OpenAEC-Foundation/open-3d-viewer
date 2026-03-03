import * as THREE from 'three';

/**
 * First-person walk mode with WASD + mouselook.
 */
export class WalkMode {
  private camera: THREE.Camera;
  private domElement: HTMLElement;
  private enabled = false;

  /** Movement speed (units/second) */
  speed = 5;

  /** Mouse sensitivity */
  sensitivity = 0.002;

  setCamera(camera: THREE.Camera): void {
    this.camera = camera;
  }

  /** Euler angles for mouselook */
  private euler = new THREE.Euler(0, 0, 0, 'YXZ');

  /** Currently pressed keys */
  private keys = new Set<string>();

  /** Is pointer locked */
  private isLocked = false;

  private clock = new THREE.Clock(false);

  private _onKeyDown: (e: KeyboardEvent) => void;
  private _onKeyUp: (e: KeyboardEvent) => void;
  private _onMouseMove: (e: MouseEvent) => void;
  private _onPointerlockChange: () => void;
  private _onClick: () => void;

  constructor(camera: THREE.Camera, domElement: HTMLElement) {
    this.camera = camera;
    this.domElement = domElement;

    this._onKeyDown = (e: KeyboardEvent) => {
      if (!this.enabled || !this.isLocked) return;
      this.keys.add(e.key.toLowerCase());
    };

    this._onKeyUp = (e: KeyboardEvent) => {
      this.keys.delete(e.key.toLowerCase());
    };

    this._onMouseMove = (e: MouseEvent) => {
      if (!this.enabled || !this.isLocked) return;
      this.euler.setFromQuaternion(this.camera.quaternion);
      this.euler.y -= e.movementX * this.sensitivity;
      this.euler.x -= e.movementY * this.sensitivity;
      // Clamp pitch
      this.euler.x = Math.max(-Math.PI / 2 + 0.01, Math.min(Math.PI / 2 - 0.01, this.euler.x));
      this.camera.quaternion.setFromEuler(this.euler);
    };

    this._onPointerlockChange = () => {
      this.isLocked = document.pointerLockElement === this.domElement;
      if (!this.isLocked) {
        this.keys.clear();
      }
    };

    this._onClick = () => {
      if (this.enabled && !this.isLocked) {
        this.domElement.requestPointerLock();
      }
    };

    document.addEventListener('keydown', this._onKeyDown);
    document.addEventListener('keyup', this._onKeyUp);
    document.addEventListener('mousemove', this._onMouseMove);
    document.addEventListener('pointerlockchange', this._onPointerlockChange);
    this.domElement.addEventListener('click', this._onClick);
  }

  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (enabled) {
      this.clock.start();
    } else {
      this.clock.stop();
      this.keys.clear();
      if (this.isLocked) {
        document.exitPointerLock();
      }
    }
  }

  update(): void {
    if (!this.enabled) return;

    const dt = this.clock.getDelta();
    if (dt === 0) return;

    const moveSpeed = this.speed * dt;

    // Direction vectors
    const forward = new THREE.Vector3();
    this.camera.getWorldDirection(forward);
    forward.y = 0;
    forward.normalize();

    const right = new THREE.Vector3();
    right.crossVectors(forward, new THREE.Vector3(0, 1, 0)).normalize();

    const movement = new THREE.Vector3();

    if (this.keys.has('w')) movement.add(forward);
    if (this.keys.has('s')) movement.sub(forward);
    if (this.keys.has('a')) movement.sub(right);
    if (this.keys.has('d')) movement.add(right);
    if (this.keys.has(' ')) movement.y += 1;       // Space = up
    if (this.keys.has('shift')) movement.y -= 1;    // Shift = down

    if (movement.lengthSq() > 0) {
      movement.normalize().multiplyScalar(moveSpeed);
      this.camera.position.add(movement);
    }
  }

  dispose(): void {
    document.removeEventListener('keydown', this._onKeyDown);
    document.removeEventListener('keyup', this._onKeyUp);
    document.removeEventListener('mousemove', this._onMouseMove);
    document.removeEventListener('pointerlockchange', this._onPointerlockChange);
    this.domElement.removeEventListener('click', this._onClick);
    if (this.isLocked) {
      document.exitPointerLock();
    }
  }
}
