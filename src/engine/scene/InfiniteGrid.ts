import * as THREE from 'three';

/**
 * InfiniteGrid — a ground grid drawn by a shader instead of THREE.GridHelper.
 *
 * - Minor and major lines (1 m / 10 m by default, ×10 for large models), anti-aliased with
 *   screen-space derivatives so they stay one smooth pixel wide at any distance.
 * - The plane follows the camera and fades out with distance, so the grid has no visible edge.
 * - It lies at the lowest level of the loaded models (fitToBounds) and takes its colours from the
 *   UI theme (OpenAEC palette).
 *
 * Scene convention: Z up, metres; the grid lies in the XY plane.
 */
export class InfiniteGrid extends THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial> {
  readonly isInfiniteGrid = true;
  private minor = 1;

  constructor() {
    const material = new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      side: THREE.DoubleSide,
      uniforms: {
        uMinor: { value: 1 },
        uMajor: { value: 10 },
        uMinorColor: { value: new THREE.Color(0xd6d3d1) },
        uMajorColor: { value: new THREE.Color(0xa8a29e) },
        uAxisColor: { value: new THREE.Color(0xd97706) },
        uOpacity: { value: 1 },
        uFadeFrom: { value: 50 },
        uFadeTo: { value: 100 },
        uCenter: { value: new THREE.Vector2() },
      },
      vertexShader: /* glsl */ `
        varying vec3 vWorld;
        void main() {
          vec4 w = modelMatrix * vec4(position, 1.0);
          vWorld = w.xyz;
          gl_Position = projectionMatrix * viewMatrix * w;
        }
      `,
      fragmentShader: /* glsl */ `
        uniform float uMinor;
        uniform float uMajor;
        uniform vec3 uMinorColor;
        uniform vec3 uMajorColor;
        uniform vec3 uAxisColor;
        uniform float uOpacity;
        uniform float uFadeFrom;
        uniform float uFadeTo;
        uniform vec2 uCenter;
        varying vec3 vWorld;

        // coverage of lines every 'size' units, one pixel wide, anti-aliased
        float lines(vec2 p, float size) {
          vec2 c = p / size;
          vec2 g = abs(fract(c - 0.5) - 0.5) / fwidth(c);
          return 1.0 - min(min(g.x, g.y), 1.0);
        }
        // a single line through the origin along one axis
        float axis(float v) {
          return 1.0 - min(abs(v) / fwidth(v), 1.0);
        }

        void main() {
          vec2 p = vWorld.xy;
          float minor = lines(p, uMinor);
          float major = lines(p, uMajor);
          float ax = max(axis(p.x), axis(p.y));
          // minor lines fade out where they get denser than a few pixels
          vec2 dens = fwidth(p) / uMinor;
          float minorFade = 1.0 - smoothstep(0.15, 0.4, max(dens.x, dens.y));
          float fade = 1.0 - smoothstep(uFadeFrom, uFadeTo, distance(p, uCenter));
          vec3 color = mix(uMinorColor, uMajorColor, major);
          color = mix(color, uAxisColor, ax);
          float alpha = max(max(minor * 0.55 * minorFade, major * 0.9), ax) * fade * uOpacity;
          if (alpha < 0.01) discard;
          gl_FragColor = vec4(color, alpha);
        }
      `,
    });
    super(new THREE.PlaneGeometry(1, 1), material);
    this.name = 'InfiniteGrid';
    this.userData.isGrid = true;
    this.renderOrder = -1;
    this.frustumCulled = false;
    // the grid never takes part in picking
    this.raycast = () => {};
  }

  /** Cell size from the size of the models, and the grid at their lowest level. */
  fitToBounds(box: THREE.Box3 | null | undefined) {
    if (!box || box.isEmpty()) {
      this.position.z = 0;
      this.setCell(1);
      return;
    }
    const size = box.getSize(new THREE.Vector3());
    const span = Math.max(size.x, size.y);
    this.position.z = box.min.z;
    this.setCell(span > 1000 ? 10 : span < 5 ? 0.1 : 1);
  }

  private setCell(minor: number) {
    this.minor = minor;
    this.material.uniforms.uMinor.value = minor;
    this.material.uniforms.uMajor.value = minor * 10;
  }

  /** Colours per UI theme: lines a little off the viewport background, the axes in amber. */
  setTheme(theme: string) {
    const palette: Record<string, [number, number, number]> = {
      light: [0xd6d3d1, 0xa8a29e, 0xd97706],
      forge: [0x3f3f46, 0x5a5a63, 0xd97706],
      openaec: [0x3a3330, 0x57504b, 0xd97706],
      blueprint: [0x1a2d47, 0x2f4d75, 0x5b9bd5],
      contrast: [0x333333, 0x666666, 0xffb000],
    };
    const [minor, major, axis] = palette[theme] ?? palette.forge;
    this.material.uniforms.uMinorColor.value.setHex(minor);
    this.material.uniforms.uMajorColor.value.setHex(major);
    this.material.uniforms.uAxisColor.value.setHex(axis);
  }

  /** Keep the plane under the camera and the fade proportional to the viewing distance. */
  update(camera: THREE.Camera) {
    const cam = camera.position;
    const height = Math.max(Math.abs(cam.z - this.position.z), this.minor);
    // orthographic cameras look from far away: use the visible width instead
    const ortho = camera as THREE.OrthographicCamera;
    const reach = ortho.isOrthographicCamera
      ? Math.max(ortho.right - ortho.left, ortho.top - ortho.bottom) / Math.max(ortho.zoom, 1e-6)
      : height;
    const fadeTo = Math.max(reach * 12, this.minor * 100);
    const u = this.material.uniforms;
    u.uFadeTo.value = fadeTo;
    u.uFadeFrom.value = fadeTo * 0.35;
    u.uCenter.value.set(cam.x, cam.y);
    // the plane spans the faded area around the camera, snapped to major cells
    const step = this.minor * 10;
    this.position.x = Math.round(cam.x / step) * step;
    this.position.y = Math.round(cam.y / step) * step;
    this.scale.set(fadeTo * 2.2, fadeTo * 2.2, 1);
  }

  dispose() {
    this.geometry.dispose();
    this.material.dispose();
  }
}
