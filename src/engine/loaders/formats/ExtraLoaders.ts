import * as THREE from 'three';
import type { LoaderResult, ThreeJSResult, ProgressCallback } from '../types';
import { finalizeScene, defaultMaterial, getExt } from './sceneUtils';

/**
 * Smaller format loaders that wrap Three.js addons or are simple enough
 * to parse directly. Each is lazy-imported from LoaderRegistry.
 */

const LIB_BASE = import.meta.env.BASE_URL;

// ----------------------------------------------------------------------------
// Rhino 3DM (rhino3dm WASM, decoded in a worker)
// ----------------------------------------------------------------------------
export async function load3DM(data: ArrayBuffer, _fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Loading Rhino engine...');
  const { Rhino3dmLoader } = await import('three/examples/jsm/loaders/3DMLoader.js');
  const loader = new Rhino3dmLoader();
  loader.setLibraryPath(`${LIB_BASE}rhino3dm/`);
  loader.setWorkerLimit(1);

  onProgress?.(0.5, 'Parsing 3DM...');
  const object = await new Promise<THREE.Object3D>((resolve, reject) => {
    loader.parse(data, resolve, reject);
  });
  loader.dispose();

  // Rhino layers become top-level groups so the layer manager can toggle them
  const layers: { name: string; visible: boolean }[] = object.userData?.layers ?? [];
  if (layers.length > 1) {
    const root = new THREE.Group();
    const byLayer = new Map<number, THREE.Group>();
    for (const child of [...object.children]) {
      const layerIdx: number = child.userData?.attributes?.layerIndex ?? -1;
      let g = byLayer.get(layerIdx);
      if (!g) {
        g = new THREE.Group();
        g.name = layers[layerIdx]?.name ?? 'Default';
        byLayer.set(layerIdx, g);
        root.add(g);
      }
      g.add(child);
    }
    return finalizeScene(root);
  }
  return finalizeScene(object);
}

// ----------------------------------------------------------------------------
// USD / USDA / USDC / USDZ
// ----------------------------------------------------------------------------
export async function loadUSD(data: ArrayBuffer, _fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Parsing USD...');
  const { USDLoader } = await import('three/examples/jsm/loaders/USDLoader.js');
  const group = new USDLoader().parse(data);
  return finalizeScene(group);
}

// ----------------------------------------------------------------------------
// Draco compressed mesh (.drc)
// ----------------------------------------------------------------------------
export async function loadDRC(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Decoding Draco...');
  const loader = await getDracoLoader();
  const geometry = await new Promise<THREE.BufferGeometry>((resolve, reject) => loader.parse(data, resolve, reject));

  let object: THREE.Object3D;
  // Draco meshes are always indexed; non-indexed output is a point cloud
  if (!geometry.index) {
    object = new THREE.Points(geometry, pointsMaterial(!!geometry.attributes.color));
  } else {
    const mat = defaultMaterial();
    if (geometry.attributes.color) { mat.vertexColors = true; mat.color.set(0xffffff); }
    object = new THREE.Mesh(geometry, mat);
  }
  object.name = fileName;
  return finalizeScene(new THREE.Group().add(object));
}

let dracoLoaderPromise: Promise<any> | null = null;
export function getDracoLoader(): Promise<any> {
  if (!dracoLoaderPromise) {
    dracoLoaderPromise = import('three/examples/jsm/loaders/DRACOLoader.js').then(({ DRACOLoader }) => {
      const loader = new DRACOLoader();
      loader.setDecoderPath(`${LIB_BASE}draco/`);
      return loader;
    });
  }
  return dracoLoaderPromise;
}

// ----------------------------------------------------------------------------
// Protein Data Bank (.pdb) — atoms as spheres, bonds as lines
// ----------------------------------------------------------------------------
export async function loadPDB(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Parsing PDB...');
  const { PDBLoader } = await import('three/examples/jsm/loaders/PDBLoader.js');
  const pdb = new PDBLoader().parse(new TextDecoder().decode(data));
  const atoms = pdb.geometryAtoms as THREE.BufferGeometry;
  const bonds = pdb.geometryBonds as THREE.BufferGeometry;

  const root = new THREE.Group();
  root.name = fileName;

  const pos = atoms.getAttribute('position');
  const col = atoms.getAttribute('color');
  const sphere = new THREE.IcosahedronGeometry(0.35, 2);
  const atomMesh = new THREE.InstancedMesh(sphere, new THREE.MeshStandardMaterial({ roughness: 0.4 }), pos.count);
  const m = new THREE.Matrix4();
  const c = new THREE.Color();
  for (let i = 0; i < pos.count; i++) {
    m.makeTranslation(pos.getX(i), pos.getY(i), pos.getZ(i));
    atomMesh.setMatrixAt(i, m);
    atomMesh.setColorAt(i, c.setRGB(col.getX(i), col.getY(i), col.getZ(i)));
  }
  atomMesh.name = 'Atoms';
  root.add(atomMesh);

  const bondLines = new THREE.LineSegments(bonds, new THREE.LineBasicMaterial({ color: 0xbbbbbb }));
  bondLines.name = 'Bonds';
  root.add(bondLines);

  const result = finalizeScene(root);
  result.totalVertices = pos.count;
  result.totalTriangles = pos.count * (sphere.index ? sphere.index.count / 3 : sphere.attributes.position.count / 3);
  return result;
}

// ----------------------------------------------------------------------------
// Quake II MD2
// ----------------------------------------------------------------------------
export async function loadMD2(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Parsing MD2...');
  const { MD2Loader } = await import('three/examples/jsm/loaders/MD2Loader.js');
  const geometry = new MD2Loader().parse(data);
  const mesh = new THREE.Mesh(geometry, defaultMaterial());
  mesh.name = fileName;
  return finalizeScene(new THREE.Group().add(mesh));
}

// ----------------------------------------------------------------------------
// BVH motion capture — show the skeleton in its rest pose
// ----------------------------------------------------------------------------
export async function loadBVH(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Parsing BVH...');
  const { BVHLoader } = await import('three/examples/jsm/loaders/BVHLoader.js');
  const { skeleton } = new BVHLoader().parse(new TextDecoder().decode(data));

  const root = new THREE.Group();
  root.name = fileName;
  root.add(skeleton.bones[0]);
  root.updateMatrixWorld(true);

  const positions: number[] = [];
  const a = new THREE.Vector3();
  const b = new THREE.Vector3();
  for (const bone of skeleton.bones) {
    if (bone.parent && (bone.parent as THREE.Bone).isBone) {
      a.setFromMatrixPosition(bone.parent.matrixWorld);
      b.setFromMatrixPosition(bone.matrixWorld);
      positions.push(a.x, a.y, a.z, b.x, b.y, b.z);
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  const lines = new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: 0x4fc3f7 }));
  lines.name = 'Skeleton';
  root.add(lines);
  return finalizeScene(root);
}

// ----------------------------------------------------------------------------
// LightWave LWO (native Three.js loader — better material support than Assimp)
// ----------------------------------------------------------------------------
export async function loadLWO(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.4, 'Parsing LWO...');
  const { LWOLoader } = await import('three/examples/jsm/loaders/LWOLoader.js');
  const { meshes } = new LWOLoader().parse(data, '', fileName);
  const root = new THREE.Group();
  for (const mesh of meshes) root.add(mesh);
  return finalizeScene(root);
}

// ----------------------------------------------------------------------------
// Point clouds: LAS (uncompressed), PTS, ASC/TXT/CSV
// ----------------------------------------------------------------------------
const MAX_POINTS = 20_000_000;

function pointsMaterial(vertexColors: boolean): THREE.PointsMaterial {
  return new THREE.PointsMaterial({ size: 2, sizeAttenuation: false, vertexColors, color: vertexColors ? 0xffffff : 0x88ccff });
}

function buildPoints(positions: Float32Array, colors: Float32Array | null, name: string): ThreeJSResult {
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  if (colors) geo.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  const points = new THREE.Points(geo, pointsMaterial(!!colors));
  points.name = name;
  return finalizeScene(new THREE.Group().add(points));
}

export async function loadLAS(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  const view = new DataView(data);
  const sig = String.fromCharCode(view.getUint8(0), view.getUint8(1), view.getUint8(2), view.getUint8(3));
  if (sig !== 'LASF') throw new Error('LAS: invalid file signature');

  const versionMinor = view.getUint8(25);
  const pointOffset = view.getUint32(96, true);
  const rawFormat = view.getUint8(104);
  if (rawFormat & 0x80 || rawFormat & 0x40) {
    throw new Error('LAZ: compressed point clouds are not supported yet — decompress to LAS first (e.g. with laszip)');
  }
  const format = rawFormat & 0x3f;
  const recordLength = view.getUint16(105, true);
  let count = view.getUint32(107, true);
  if (count === 0 && versionMinor >= 4) count = Number(view.getBigUint64(247, true));

  const sx = view.getFloat64(131, true), sy = view.getFloat64(139, true), sz = view.getFloat64(147, true);
  const ox = view.getFloat64(155, true), oy = view.getFloat64(163, true), oz = view.getFloat64(171, true);
  const minX = view.getFloat64(187, true), minY = view.getFloat64(203, true), minZ = view.getFloat64(219, true);

  const rgbOffset = ({ 2: 20, 3: 28, 5: 28, 7: 30, 8: 30, 10: 30 } as Record<number, number>)[format];
  const available = Math.floor((data.byteLength - pointOffset) / recordLength);
  count = Math.min(count, available);
  const step = Math.max(1, Math.ceil(count / MAX_POINTS));
  const n = Math.floor(count / step);

  const positions = new Float32Array(n * 3);
  const colors = new Float32Array(n * 3);
  let maxRgb = 0;
  let maxIntensity = 1;

  for (let i = 0; i < n; i++) {
    const p = pointOffset + i * step * recordLength;
    // Positions relative to the header minimum (LAS is Z-up)
    positions[i * 3] = view.getInt32(p, true) * sx + ox - minX;
    positions[i * 3 + 1] = view.getInt32(p + 8, true) * sz + oz - minZ;
    positions[i * 3 + 2] = -(view.getInt32(p + 4, true) * sy + oy - minY);
    if (rgbOffset !== undefined) {
      const r = view.getUint16(p + rgbOffset, true), g = view.getUint16(p + rgbOffset + 2, true), b = view.getUint16(p + rgbOffset + 4, true);
      colors[i * 3] = r; colors[i * 3 + 1] = g; colors[i * 3 + 2] = b;
      maxRgb = Math.max(maxRgb, r, g, b);
    } else {
      const intensity = view.getUint16(p + 12, true);
      colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = intensity;
      maxIntensity = Math.max(maxIntensity, intensity);
    }
    if (i % 500_000 === 0) onProgress?.(0.2 + 0.7 * (i / n), `Reading points (${i.toLocaleString()}/${n.toLocaleString()})...`);
  }

  // RGB may be 8-bit or 16-bit depending on the producer
  const scale = rgbOffset !== undefined ? (maxRgb > 255 ? 65535 : 255) : maxIntensity;
  const hasColor = rgbOffset !== undefined ? maxRgb > 0 : maxIntensity > 1;
  for (let i = 0; i < colors.length; i++) colors[i] /= scale;

  return buildPoints(positions, hasColor ? colors : null, fileName);
}

/** Whitespace/comma separated text point clouds: x y z [intensity] [r g b] */
export async function loadTextPoints(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing points...');
  const lines = new TextDecoder().decode(data).split(/\r?\n/);
  const pos: number[] = [];
  const col: number[] = [];
  let hasColor = true;
  for (const line of lines) {
    const parts = line.trim().split(/[\s,;]+/);
    if (parts.length < 3) continue;
    const x = parseFloat(parts[0]), y = parseFloat(parts[1]), z = parseFloat(parts[2]);
    if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) continue;
    pos.push(x, z, -y);
    // PTS: x y z i r g b ; ASC/TXT often: x y z r g b
    const rgb = parts.length >= 7 ? parts.slice(4, 7) : parts.length === 6 ? parts.slice(3, 6) : null;
    if (rgb) col.push(+rgb[0] / 255, +rgb[1] / 255, +rgb[2] / 255);
    else hasColor = false;
  }
  if (pos.length === 0) throw new Error(`${getExt(fileName).toUpperCase()}: no points found`);
  return buildPoints(new Float32Array(pos), hasColor && col.length === pos.length ? new Float32Array(col) : null, fileName);
}

// ----------------------------------------------------------------------------
// ZIP archives — open the main model inside (e.g. OBJ + textures, zipped IFC)
// ----------------------------------------------------------------------------
export async function loadZIP(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<LoaderResult> {
  onProgress?.(0.2, 'Unpacking archive...');
  const { unzipSync } = await import('fflate');
  const { getLoader, isSupported } = await import('../LoaderRegistry');
  const entries = unzipSync(new Uint8Array(data));

  const candidates = Object.entries(entries)
    .filter(([name, bytes]) => !name.endsWith('/') && bytes.length > 0 && !name.startsWith('__MACOSX'))
    .map(([name, bytes]) => ({ name, bytes, ext: '.' + getExt(name) }))
    .filter((e) => isSupported(e.ext) && e.ext !== '.zip' && e.ext !== '.ifczip')
    .sort((a, b) => b.bytes.length - a.bytes.length);

  if (candidates.length === 0) throw new Error(`${fileName}: the archive contains no supported 3D file`);

  const main = candidates[0];
  const loader = await getLoader(main.ext);
  if (!loader) throw new Error(`${fileName}: no loader for ${main.ext}`);
  const buffer = main.bytes.buffer.slice(main.bytes.byteOffset, main.bytes.byteOffset + main.bytes.byteLength) as ArrayBuffer;
  return loader(buffer, main.name.split('/').pop() || main.name, onProgress);
}
