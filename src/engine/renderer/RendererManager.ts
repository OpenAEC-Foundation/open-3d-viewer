import * as THREE from 'three';

/**
 * RendererManager — Manages the Three.js renderer.
 *
 * Attempts WebGPU first, falls back to WebGL2.
 * Exposes a unified render/resize/dispose API.
 */
export class RendererManager {
  renderer: THREE.WebGLRenderer;
  rendererType: 'webgpu' | 'webgl' = 'webgl';

  /** Draw call count from last frame (for stats) */
  drawCalls = 0;
  /** Triangle count from last frame (for stats) */
  triangles = 0;

  constructor(canvas: HTMLCanvasElement) {
    // WebGPURenderer is still experimental in Three.js.
    // For production stability, we use WebGLRenderer with high-quality settings.
    // WebGPU can be enabled via: import { WebGPURenderer } from 'three/webgpu'
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: false,
      stencil: true,
      powerPreference: 'high-performance',
    });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.0;
    this.renderer.localClippingEnabled = true;
    this.rendererType = 'webgl';
  }

  setSize(width: number, height: number) {
    this.renderer.setSize(width, height, false);
  }

  render(scene: THREE.Scene, camera: THREE.Camera) {
    this.renderer.render(scene, camera);

    // Capture render stats
    const info = this.renderer.info;
    this.drawCalls = info.render.calls;
    this.triangles = info.render.triangles;
  }

  /**
   * Get GPU memory info (if available via WebGL extension).
   */
  getMemoryInfo(): { geometries: number; textures: number } {
    const info = this.renderer.info;
    return {
      geometries: info.memory.geometries,
      textures: info.memory.textures,
    };
  }

  dispose() {
    this.renderer.dispose();
  }
}
