import * as THREE from 'three';

/**
 * First-person walk mode — Unity/Unreal style.
 *
 * Hold right-click to look around (cursor hidden, manual delta tracking).
 * While looking: WASD to move, Shift to sprint, Q/E or Space/C for vertical.
 * Scroll wheel adjusts walk speed anytime walk mode is active.
 */
export class WalkMode {
  private camera: THREE.Camera;
  private domElement: HTMLElement;
  private enabled = false;

  /** Movement speed (units/second) */
  speed = 5;

  /** Mouse sensitivity (radians/pixel) */
  sensitivity = 0.002;

  /** Called when scroll wheel adjusts speed */
  onSpeedChange?: (speed: number) => void;

  setCamera(camera: THREE.Camera): void {
    this.camera = camera;
  }

  // Euler angles for mouselook (Z-up: yaw around Z, pitch around local X)
  private euler = new THREE.Euler(0, 0, 0, 'ZXY');

  // Reusable vectors to avoid per-frame allocations
  private _forward = new THREE.Vector3();
  private _right = new THREE.Vector3();
  private _movement = new THREE.Vector3();

  private keys = new Set<string>();
  private isLooking = false;
  private lastMouseX = 0;
  private lastMouseY = 0;
  private clock = new THREE.Clock(false);

  // Event handler references
  private _onKeyDown: (e: KeyboardEvent) => void;
  private _onKeyUp: (e: KeyboardEvent) => void;
  private _onMouseMove: (e: MouseEvent) => void;
  private _onMouseDown: (e: MouseEvent) => void;
  private _onMouseUp: (e: MouseEvent) => void;
  private _onContextMenu: (e: Event) => void;
  private _onWheel: (e: WheelEvent) => void;

  constructor(camera: THREE.Camera, domElement: HTMLElement) {
    this.camera = camera;
    this.domElement = domElement;

    this._onKeyDown = (e: KeyboardEvent) => {
      if (!this.enabled || !this.isLooking) return;
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      const key = e.key.toLowerCase();
      this.keys.add(key);
      if (['w', 'a', 's', 'd', 'q', 'e', ' ', 'arrowup', 'arrowdown', 'arrowleft', 'arrowright'].includes(key)) {
        e.preventDefault();
      }
    };

    this._onKeyUp = (e: KeyboardEvent) => {
      this.keys.delete(e.key.toLowerCase());
    };

    this._onMouseMove = (e: MouseEvent) => {
      if (!this.enabled || !this.isLooking) return;
      const dx = e.clientX - this.lastMouseX;
      const dy = e.clientY - this.lastMouseY;
      this.lastMouseX = e.clientX;
      this.lastMouseY = e.clientY;

      this.euler.setFromQuaternion(this.camera.quaternion);
      this.euler.z -= dx * this.sensitivity;  // yaw around Z (up)
      this.euler.x -= dy * this.sensitivity;  // pitch around local X
      // Clamp pitch to avoid flipping
      this.euler.x = Math.max(-Math.PI / 2 + 0.01, Math.min(Math.PI / 2 - 0.01, this.euler.x));
      this.camera.quaternion.setFromEuler(this.euler);
    };

    this._onMouseDown = (e: MouseEvent) => {
      if (!this.enabled || e.button !== 2) return;
      e.preventDefault();
      this.isLooking = true;
      this.lastMouseX = e.clientX;
      this.lastMouseY = e.clientY;
      this.domElement.style.cursor = 'none';
    };

    this._onMouseUp = (e: MouseEvent) => {
      if (e.button !== 2) return;
      this.isLooking = false;
      this.keys.clear();
      if (this.enabled) {
        this.domElement.style.cursor = '';
      }
    };

    this._onContextMenu = (e: Event) => {
      if (this.enabled) e.preventDefault();
    };

    this._onWheel = (e: WheelEvent) => {
      if (!this.enabled) return;
      e.preventDefault();
      const factor = e.deltaY > 0 ? 0.85 : 1.18;
      this.speed = Math.max(0.5, Math.min(50, this.speed * factor));
      this.onSpeedChange?.(this.speed);
    };

    document.addEventListener('keydown', this._onKeyDown);
    document.addEventListener('keyup', this._onKeyUp);
    document.addEventListener('mousemove', this._onMouseMove);
    this.domElement.addEventListener('mousedown', this._onMouseDown);
    document.addEventListener('mouseup', this._onMouseUp);
    this.domElement.addEventListener('contextmenu', this._onContextMenu);
    this.domElement.addEventListener('wheel', this._onWheel, { passive: false });
  }

  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (enabled) {
      this.clock.start();
      // Initialize euler from current camera orientation
      this.euler.setFromQuaternion(this.camera.quaternion);
    } else {
      this.clock.stop();
      this.keys.clear();
      this.isLooking = false;
      this.domElement.style.cursor = '';
    }
  }

  update(): void {
    if (!this.enabled) return;

    // Consume delta every frame to avoid large jump when looking starts
    const dt = this.clock.getDelta();

    if (!this.isLooking || dt === 0) return;

    const sprint = this.keys.has('shift') ? 2.5 : 1;
    const moveSpeed = this.speed * sprint * dt;

    // Use yaw angle for ground-plane directions (robust when looking straight up/down)
    const yaw = this.euler.z;
    const forward = this._forward.set(-Math.sin(yaw), Math.cos(yaw), 0);
    const right = this._right.set(Math.cos(yaw), Math.sin(yaw), 0);

    const movement = this._movement.set(0, 0, 0);

    // WASD + Arrow keys
    if (this.keys.has('w') || this.keys.has('arrowup')) movement.add(forward);
    if (this.keys.has('s') || this.keys.has('arrowdown')) movement.sub(forward);
    if (this.keys.has('a') || this.keys.has('arrowleft')) movement.sub(right);
    if (this.keys.has('d') || this.keys.has('arrowright')) movement.add(right);

    // Vertical: Q/E or Space/C
    if (this.keys.has('e') || this.keys.has(' ')) movement.z += 1;
    if (this.keys.has('q') || this.keys.has('c')) movement.z -= 1;

    if (movement.lengthSq() > 0) {
      movement.normalize().multiplyScalar(moveSpeed);
      this.camera.position.add(movement);
    }
  }

  dispose(): void {
    document.removeEventListener('keydown', this._onKeyDown);
    document.removeEventListener('keyup', this._onKeyUp);
    document.removeEventListener('mousemove', this._onMouseMove);
    this.domElement.removeEventListener('mousedown', this._onMouseDown);
    document.removeEventListener('mouseup', this._onMouseUp);
    this.domElement.removeEventListener('contextmenu', this._onContextMenu);
    this.domElement.removeEventListener('wheel', this._onWheel);
    this.domElement.style.cursor = '';
  }
}
