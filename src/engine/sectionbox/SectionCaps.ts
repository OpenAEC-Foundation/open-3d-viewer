import * as THREE from 'three';
import type { SectionBox } from './SectionBox';

/** Layer the model meshes are on (besides 0) so the stencil passes draw only them. */
export const CAP_LAYER = 5;

/**
 * SectionCaps — closes the solids the section box cuts open (OpenAEC design: open-bim-studio
 * `bim_section_caps`, cut faces with hatching on a 200 mm world grid).
 *
 * Stencil technique, after the normal frame: for each box face that cuts the models, the model
 * meshes are drawn twice into the stencil buffer only, clipped by that face (back faces +1, front
 * faces −1); where a solid is cut open the count stays non-zero. A cap quad on the face, trimmed by
 * the other five faces, is drawn there with a fill and a 45° hatch, and resets the stencil.
 * Needs a renderer created with `stencil: true`. Open (non-closed) meshes give partial caps.
 */
export class SectionCaps {
  private readonly backMats: THREE.MeshBasicMaterial[] = [];
  private readonly frontMats: THREE.MeshBasicMaterial[] = [];
  private readonly caps: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>[] = [];
  private readonly capScene = new THREE.Scene();
  private readonly layers = new THREE.Layers();

  constructor() {
    this.layers.set(CAP_LAYER);
    for (let i = 0; i < 6; i++) {
      const stencil = (side: THREE.Side, op: THREE.StencilOp) =>
        new THREE.MeshBasicMaterial({
          side,
          colorWrite: false,
          depthWrite: false,
          depthTest: false,
          stencilWrite: true,
          stencilFunc: THREE.AlwaysStencilFunc,
          stencilFail: op,
          stencilZFail: op,
          stencilZPass: op,
        });
      this.backMats.push(stencil(THREE.BackSide, THREE.IncrementWrapStencilOp));
      this.frontMats.push(stencil(THREE.FrontSide, THREE.DecrementWrapStencilOp));
      const cap = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), SectionCaps.capMaterial(i));
      cap.frustumCulled = false;
      cap.visible = false;
      this.caps.push(cap);
      this.capScene.add(cap);
    }
    this.setTheme('light');
  }

  /** Fill and hatch colours per UI theme. */
  setTheme(theme: string) {
    const dark = theme !== 'light';
    for (const cap of this.caps) {
      cap.material.uniforms.uFill.value.setHex(dark ? 0x57534e : 0xd6d3d1);
      cap.material.uniforms.uLine.value.setHex(dark ? 0x292524 : 0x78716c);
    }
  }

  /**
   * Draw the caps of the current frame (call right after the scene was rendered, same camera).
   * `bounds`: the extent of the models; faces outside it cut nothing and are skipped.
   */
  render(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, box: SectionBox, bounds: THREE.Box3) {
    if (!box.enabled || bounds.isEmpty()) return;
    const planes = box.planes;
    const { min, max } = box;
    // plane i: axis i >> 1, at max (even i) or min (odd i)
    const active: number[] = [];
    for (let i = 0; i < 6; i++) {
      const axis = i >> 1;
      const at = i % 2 === 0 ? max.getComponent(axis) : min.getComponent(axis);
      if (at > bounds.min.getComponent(axis) && at < bounds.max.getComponent(axis)) active.push(i);
    }
    if (active.length === 0) return;

    const autoClear = renderer.autoClear;
    const shadows = renderer.shadowMap.enabled;
    const background = scene.background;
    const cameraLayers = camera.layers.mask;
    renderer.autoClear = false;
    renderer.shadowMap.enabled = false;
    scene.background = null;
    try {
      for (const i of active) {
        const axis = i >> 1;
        const others = planes.filter((_, k) => k !== i);
        // stencil count of the solids cut open by face i
        camera.layers.mask = this.layers.mask;
        for (const mat of [this.backMats[i], this.frontMats[i]]) {
          mat.clippingPlanes = [planes[i]];
          scene.overrideMaterial = mat;
          renderer.render(scene, camera);
        }
        scene.overrideMaterial = null;
        camera.layers.mask = cameraLayers;
        // the cap quad on face i, trimmed by the other faces
        const cap = this.caps[i];
        const size = new THREE.Vector3().subVectors(max, min);
        const center = new THREE.Vector3().addVectors(min, max).multiplyScalar(0.5);
        center.setComponent(axis, i % 2 === 0 ? max.getComponent(axis) : min.getComponent(axis));
        cap.position.copy(center);
        cap.rotation.set(0, 0, 0);
        if (axis === 0) cap.rotation.y = Math.PI / 2;
        if (axis === 1) cap.rotation.x = Math.PI / 2;
        const [w, h] = axis === 0 ? [size.z, size.y] : axis === 1 ? [size.x, size.z] : [size.x, size.y];
        cap.scale.set(w * 1.001 + 1e-3, h * 1.001 + 1e-3, 1);
        cap.material.clippingPlanes = others;
        for (const c of this.caps) c.visible = c === cap;
        renderer.render(this.capScene, camera);
        cap.visible = false;
      }
    } finally {
      scene.overrideMaterial = null;
      camera.layers.mask = cameraLayers;
      scene.background = background;
      renderer.shadowMap.enabled = shadows;
      renderer.autoClear = autoClear;
    }
  }

  dispose() {
    for (const m of [...this.backMats, ...this.frontMats]) m.dispose();
    for (const c of this.caps) {
      c.geometry.dispose();
      c.material.dispose();
    }
  }

  /** Fill with a 45° hatch every 200 mm in world coordinates of the face's plane. */
  private static capMaterial(i: number): THREE.ShaderMaterial {
    return new THREE.ShaderMaterial({
      side: THREE.DoubleSide,
      clipping: true,
      stencilWrite: true,
      stencilRef: 0,
      stencilFunc: THREE.NotEqualStencilFunc,
      stencilFail: THREE.ReplaceStencilOp,
      stencilZFail: THREE.ReplaceStencilOp,
      stencilZPass: THREE.ReplaceStencilOp,
      uniforms: {
        uFill: { value: new THREE.Color() },
        uLine: { value: new THREE.Color() },
        uSpacing: { value: 0.2 },
        uAxis: { value: i >> 1 },
      },
      vertexShader: /* glsl */ `
        #include <clipping_planes_pars_vertex>
        varying vec3 vWorld;
        void main() {
          vec4 mvPosition = modelViewMatrix * vec4(position, 1.0);
          vWorld = (modelMatrix * vec4(position, 1.0)).xyz;
          gl_Position = projectionMatrix * mvPosition;
          #include <clipping_planes_vertex>
        }
      `,
      fragmentShader: /* glsl */ `
        #include <clipping_planes_pars_fragment>
        uniform vec3 uFill;
        uniform vec3 uLine;
        uniform float uSpacing;
        uniform int uAxis;
        varying vec3 vWorld;
        void main() {
          #include <clipping_planes_fragment>
          vec2 p = uAxis == 0 ? vWorld.yz : (uAxis == 1 ? vWorld.xz : vWorld.xy);
          float t = (p.x + p.y) / uSpacing;
          float line = 1.0 - min(abs(fract(t - 0.5) - 0.5) / fwidth(t), 1.0);
          gl_FragColor = vec4(mix(uFill, uLine, line * 0.8), 1.0);
        }
      `,
    });
  }
}
