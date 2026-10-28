import * as THREE from 'three';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';
import { t } from '../../i18n/useTranslation';
import { worldPerPixel } from '../sectionplane/SectionPlaneGizmo';
import { getFeatureLines } from './meshTopology';
import {
  objectWorldMatrix,
  type AngleResult, type DistanceResult, type FaceRegion, type PolygonResult, type VolumeResult,
} from './measureMath';
import type { SnapKind, SnapResult } from './MeasureSnapper';
import { densityFor, type ObjectPart, type WeightSettings } from './measureObjects';
import {
  DEFAULT_UNIT_SETTINGS, formatCoordinate, formatDensity, formatValue,
  type MeasureCategory, type UnitSettings,
} from './measureFormat';
import type { MeasurementSummary } from '../../state/slices/measureSlice';

/**
 * Measurements and their visuals: lines and markers in the scene (drawn on
 * top of the model) and HTML labels positioned over the canvas.
 *
 * Measurements are kept as plain data and rebuilt whenever the display
 * settings change (units, decimals, XYZ legs, densities).
 */

/** What a pick snapped to, kept without references to scene objects */
export interface Highlight {
  kind: SnapKind;
  point: THREE.Vector3;
  edge?: { a: THREE.Vector3; b: THREE.Vector3 };
  normal: THREE.Vector3;
}

export interface ObjectDescription {
  name: string;
  material?: string;
  /** Weight stated in the file (kg) */
  fileWeight?: number;
}

export type MeasurementData =
  | { type: 'point'; p: THREE.Vector3 }
  | { type: 'distance'; result: DistanceResult; perpendicular: boolean; from: Highlight; to: Highlight }
  | { type: 'angle'; result: AngleResult; radius: number; three: boolean; from?: Highlight; to?: Highlight }
  | { type: 'area'; region: FaceRegion; object: ObjectDescription }
  | { type: 'polygon'; points: THREE.Vector3[]; result: PolygonResult }
  | { type: 'object'; method: 'volume' | 'areaTotal' | 'weight'; result: VolumeResult; outline: Float32Array; object: ObjectDescription }
  | {
    type: 'count'; method: 'count' | 'countEntity'; key: string; p: THREE.Vector3;
    /** Entity kind for entity counts */
    kind?: SnapKind;
    edge?: { a: THREE.Vector3; b: THREE.Vector3 };
    /** Object outline or face triangles to keep highlighted */
    outline?: Float32Array;
    faceTriangles?: Float32Array;
    object?: ObjectDescription;
  };

export interface DisplaySettings {
  units: UnitSettings;
  showComponents: boolean;
  weight: WeightSettings;
}

interface Label {
  el: HTMLDivElement;
  pos: THREE.Vector3;
}

interface Built {
  group: THREE.Group;
  labels: Label[];
}

interface Measurement extends Built {
  id: number;
  data: MeasurementData;
  saved: boolean;
}

const COLOR_MAIN = 0xffc400;
const COLOR_HOVER = 0x00e5ff;
/** Second entity of a two-pick measurement */
const COLOR_TO = 0xff6ad5;
const COLOR_X = 0xff5252;
const COLOR_Y = 0x4cd964;
const COLOR_Z = 0x4d94ff;
/** Size of the face square in screen pixels */
const FACE_MARKER_PIXELS = 36;

export class MeasureManager {
  /** Scene group with all measure visuals (excluded from clipping) */
  readonly group = new THREE.Group();
  private measurementsGroup = new THREE.Group();
  private previewGroup = new THREE.Group();
  private hoverGroup = new THREE.Group();
  private pendingGroup = new THREE.Group();

  private overlay: HTMLElement;
  private snapIndicator: HTMLDivElement;
  private snapPos: THREE.Vector3 | null = null;
  private previewLabels: Label[] = [];

  private measurements: Measurement[] = [];
  private nextId = 1;
  private settings: DisplaySettings = {
    units: DEFAULT_UNIT_SETTINGS,
    showComponents: true,
    weight: { materialDensities: {}, defaultDensity: 2400 },
  };
  /** Keep consecutive measurements (otherwise a new one replaces the unsaved ones) */
  multi = false;

  private materials: LineMaterial[] = [];
  private mat: Record<'main' | 'thin' | 'x' | 'y' | 'z' | 'hover' | 'to' | 'outline', LineMaterial>;
  private pointMat: THREE.PointsMaterial;
  private hoverPointMat: THREE.PointsMaterial;
  private toPointMat: THREE.PointsMaterial;
  private fillMat: THREE.MeshBasicMaterial;
  private hoverFillMat: THREE.MeshBasicMaterial;

  /** Called whenever measurements are added, removed or relabelled */
  onChange?: (summaries: MeasurementSummary[]) => void;

  constructor(scene: THREE.Scene, overlay: HTMLElement) {
    this.overlay = overlay;
    this.group.name = 'measure-group';
    this.group.add(this.measurementsGroup, this.previewGroup, this.pendingGroup, this.hoverGroup);
    scene.add(this.group);

    const line = (color: number, linewidth: number, opacity = 1) => {
      const m = new LineMaterial({
        color, linewidth, transparent: true, opacity,
        depthTest: false, depthWrite: false, toneMapped: false,
      });
      this.materials.push(m);
      return m;
    };
    this.mat = {
      main: line(COLOR_MAIN, 2.5),
      thin: line(COLOR_MAIN, 1.25, 0.7),
      x: line(COLOR_X, 1.5, 0.9),
      y: line(COLOR_Y, 1.5, 0.9),
      z: line(COLOR_Z, 1.5, 0.9),
      hover: line(COLOR_HOVER, 3),
      to: line(COLOR_TO, 3),
      outline: line(COLOR_HOVER, 1.75, 0.9),
    };
    const points = (color: number, size: number) => new THREE.PointsMaterial({
      color, size, sizeAttenuation: false, depthTest: false, depthWrite: false, transparent: true, toneMapped: false,
    });
    this.pointMat = points(COLOR_MAIN, 7);
    this.hoverPointMat = points(COLOR_HOVER, 9);
    this.toPointMat = points(COLOR_TO, 9);
    const fill = (color: number, opacity: number) => new THREE.MeshBasicMaterial({
      color, transparent: true, opacity, side: THREE.DoubleSide, depthWrite: false, toneMapped: false,
      polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
    });
    this.fillMat = fill(COLOR_HOVER, 0.28);
    this.hoverFillMat = fill(COLOR_HOVER, 0.18);

    this.snapIndicator = document.createElement('div');
    this.snapIndicator.className = 'measure-snap';
    this.snapIndicator.style.display = 'none';
    overlay.appendChild(this.snapIndicator);
  }

  setSettings(settings: Partial<DisplaySettings>): void {
    this.settings = { ...this.settings, ...settings };
    this.rebuild();
  }

  // ============================================================================
  // Measurements
  // ============================================================================

  add(data: MeasurementData): void {
    // Single selection: a new measurement replaces the unsaved ones (counting always adds up)
    if (!this.multi && data.type !== 'count') {
      for (const m of this.measurements.filter((m) => !m.saved && m.data.type !== 'count')) this.detach(m);
      this.measurements = this.measurements.filter((m) => m.saved || m.data.type === 'count');
    }
    const id = this.nextId++;
    this.measurements.push({ id, data, saved: false, group: new THREE.Group(), labels: [] });
    this.rebuild();
  }

  /** Id of the count marker with this key, if any */
  findCount(key: string): number | null {
    const m = this.measurements.find((m) => m.data.type === 'count' && m.data.key === key);
    return m ? m.id : null;
  }

  remove(id: number): void {
    const index = this.measurements.findIndex((m) => m.id === id);
    if (index < 0) return;
    const [m] = this.measurements.splice(index, 1);
    this.detach(m);
    this.rebuild();
  }

  removeLast(): void {
    const last = this.measurements[this.measurements.length - 1];
    if (last) this.remove(last.id);
  }

  clear(): void {
    for (const m of this.measurements) this.detach(m);
    this.measurements = [];
    this.emitChange();
  }

  /** Mark measurements as saved to the takeoff: they stay as markups and are no longer replaced */
  markSaved(ids: number[]): void {
    const set = new Set(ids);
    for (const m of this.measurements) if (set.has(m.id)) m.saved = true;
    this.rebuild();
  }

  /** Rebuild all visuals and labels (settings changed, or count numbering shifted) */
  private rebuild(): void {
    for (const m of this.measurements) {
      this.detach(m);
      Object.assign(m, this.build(m.data, true, m.id));
      this.attach(m);
    }
    this.emitChange();
  }

  private attach(m: Measurement): void {
    this.measurementsGroup.add(m.group);
    for (const l of m.labels) {
      l.el.classList.toggle('saved', m.saved);
      this.overlay.appendChild(l.el);
    }
  }

  private detach(m: Built): void {
    disposeGroup(m.group);
    for (const l of m.labels) l.el.remove();
  }

  private emitChange(): void {
    this.onChange?.(this.measurements.map((m) => this.summarize(m)));
  }

  private summarize(m: Measurement): MeasurementSummary {
    const d = m.data;
    const base = { id: m.id, saved: m.saved };
    switch (d.type) {
      case 'point':
        return { ...base, category: 'length', method: 'point', value: 0, detail: `${d.p.x}|${d.p.y}|${d.p.z}` };
      case 'distance':
        return { ...base, category: 'length', method: d.perpendicular ? 'perpendicular' : 'distance', value: d.result.p1.distanceTo(d.result.p2) };
      case 'angle':
        return { ...base, category: 'angle', method: d.three ? 'angle3' : 'angle', value: d.result.angle };
      case 'area':
        return {
          ...base, category: 'area', method: 'area', value: d.region.area,
          objectName: d.object.name, material: d.object.material, detail: d.region.curved ? 'curved' : undefined,
        };
      case 'polygon':
        return { ...base, category: 'area', method: 'polygon', value: d.result.area };
      case 'object': {
        const common = { ...base, objectName: d.object.name, material: d.object.material };
        if (d.method === 'volume') return { ...common, category: 'volume', method: 'volume', value: d.result.volume };
        if (d.method === 'areaTotal') return { ...common, category: 'area', method: 'areaTotal', value: d.result.surfaceArea };
        const w = this.weightOf(d);
        return { ...common, category: 'weight', method: 'weight', value: w.mass, detail: w.detail };
      }
      case 'count':
        return {
          ...base, category: 'count', method: d.method, value: 1,
          objectName: d.object?.name ?? (d.kind ? t(`ribbon:measure.snap.${d.kind}`) : undefined),
          material: d.object?.material,
          detail: d.kind,
        };
    }
  }

  /** Mass of an object: the weight from the file, or volume × density */
  private weightOf(d: Extract<MeasurementData, { type: 'object' }>): { mass: number; density?: number; detail: string } {
    if (d.object.fileWeight !== undefined) {
      return { mass: d.object.fileWeight, detail: t('ribbon:measure.weightSource.file') };
    }
    const { density, source } = densityFor(d.object.material, this.settings.weight);
    return {
      mass: d.result.volume * density,
      density,
      detail: `ρ ${formatDensity(density)} (${t(`ribbon:measure.weightSource.${source}`)})`,
    };
  }

  // ============================================================================
  // Preview, pending pick and hover
  // ============================================================================

  /** Live measurement following the cursor (not yet placed) */
  setPreview(data: MeasurementData | null): void {
    disposeChildren(this.previewGroup);
    for (const l of this.previewLabels) l.el.remove();
    this.previewLabels = [];
    if (!data) return;
    const built = this.build(data, false);
    this.previewGroup.add(built.group);
    this.previewLabels = built.labels;
    for (const l of built.labels) {
      l.el.classList.add('preview');
      this.overlay.appendChild(l.el);
    }
  }

  /** Keep the first pick of a two-pick measurement highlighted */
  setPending(snap: Highlight | null): void {
    disposeChildren(this.pendingGroup);
    if (snap) this.pendingGroup.add(this.buildHighlight(snap, false));
  }

  /** Highlight what the cursor would snap to */
  showSnap(snap: SnapResult | null, second = false): void {
    disposeChildren(this.hoverGroup);
    if (!snap) {
      this.hideSnapIndicator();
      return;
    }
    this.hoverGroup.add(this.buildHighlight(snap, second));
    this.snapPos = snap.point.clone();
    this.snapIndicator.className = `measure-snap measure-snap-${snap.kind}${second ? ' second' : ''}`;
    this.snapIndicator.dataset.label = t(`ribbon:measure.snap.${snap.kind}`);
    this.snapIndicator.style.display = '';
  }

  /** Highlight a face (area tool, face counting) */
  showFaceHover(region: FaceRegion | null): void {
    disposeChildren(this.hoverGroup);
    this.hideSnapIndicator();
    if (!region) return;
    this.hoverGroup.add(makeMesh(region.triangles, this.hoverFillMat));
    this.hoverGroup.add(makeLines(region.outline, this.mat.outline));
  }

  /** Highlight the edges of an object (volume, weight and count tools) */
  showObjectHover(outline: Float32Array | null): void {
    disposeChildren(this.hoverGroup);
    this.hideSnapIndicator();
    if (outline && outline.length > 0) this.hoverGroup.add(makeLines(outline, this.mat.outline));
  }

  clearHover(): void {
    this.showSnap(null);
  }

  clearTransient(): void {
    this.setPreview(null);
    this.setPending(null);
    this.clearHover();
  }

  private hideSnapIndicator(): void {
    this.snapPos = null;
    this.snapIndicator.style.display = 'none';
  }

  /** Visual for a snapped point / edge / face; `second` uses the colour of the second pick */
  private buildHighlight(snap: Highlight, second: boolean): THREE.Group {
    const g = new THREE.Group();
    const lineMat = second ? this.mat.to : this.mat.hover;
    if (snap.edge && snap.kind !== 'vertex') {
      g.add(makeLines([...snap.edge.a.toArray(), ...snap.edge.b.toArray()], lineMat));
    }
    if (snap.kind === 'face') {
      const square = new THREE.Group();
      square.add(makeLines(unitSquare(snap.normal), lineMat));
      square.position.copy(snap.point);
      square.userData.screenPixels = FACE_MARKER_PIXELS;
      g.add(square);
    }
    g.add(makePoints([snap.point], second ? this.toPointMat : this.hoverPointMat));
    return g;
  }

  // ============================================================================
  // Per-frame update
  // ============================================================================

  update(camera: THREE.Camera, width: number, height: number): void {
    for (const m of this.materials) m.resolution.set(width, height);

    // Keep face squares a constant size on screen
    for (const g of [this.hoverGroup, this.pendingGroup, this.previewGroup, this.measurementsGroup]) {
      g.traverse((o) => {
        const pixels = o.userData.screenPixels as number | undefined;
        if (pixels) o.scale.setScalar(worldPerPixel(camera, o.position, height) * pixels);
      });
    }

    const ndc = new THREE.Vector3();
    const place = (el: HTMLElement, pos: THREE.Vector3) => {
      ndc.copy(pos).project(camera);
      if (ndc.z < -1 || ndc.z > 1) {
        el.style.visibility = 'hidden';
        return;
      }
      el.style.visibility = '';
      const x = (ndc.x + 1) / 2 * width;
      const y = (1 - ndc.y) / 2 * height;
      el.style.transform = `translate(${x.toFixed(1)}px, ${y.toFixed(1)}px) translate(-50%, -50%)`;
    };

    for (const m of this.measurements) for (const l of m.labels) place(l.el, l.pos);
    for (const l of this.previewLabels) place(l.el, l.pos);
    if (this.snapPos) place(this.snapIndicator, this.snapPos);
  }

  /** World-space edge outline of an object, for highlighting it. */
  objectOutline(parts: ObjectPart[]): Float32Array {
    const chunks: Float32Array[] = [];
    const v = new THREE.Vector3();
    for (const { mesh, instanceId } of parts) {
      const { lines } = getFeatureLines(mesh.geometry);
      const matrix = objectWorldMatrix(mesh, instanceId);
      const out = new Float32Array(lines.length);
      for (let i = 0; i < lines.length; i += 3) v.fromArray(lines, i).applyMatrix4(matrix).toArray(out, i);
      chunks.push(out);
    }
    if (chunks.length === 1) return chunks[0];
    const all = new Float32Array(chunks.reduce((n, c) => n + c.length, 0));
    let offset = 0;
    for (const c of chunks) { all.set(c, offset); offset += c.length; }
    return all;
  }

  dispose(): void {
    this.clear();
    this.clearTransient();
    this.group.parent?.remove(this.group);
    this.snapIndicator.remove();
    for (const m of this.materials) m.dispose();
    this.pointMat.dispose();
    this.hoverPointMat.dispose();
    this.toPointMat.dispose();
    this.fillMat.dispose();
    this.hoverFillMat.dispose();
  }

  // ============================================================================
  // Building visuals
  // ============================================================================

  private fmt(category: MeasureCategory, value: number): string {
    return formatValue(category, value, this.settings.units);
  }

  private build(data: MeasurementData, placed: boolean, id = -1): Built {
    const group = new THREE.Group();
    const labels: Label[] = [];
    const units = this.settings.units;
    const onDelete = placed ? () => this.remove(id) : undefined;
    const lengthUnit = units.length.unit;

    switch (data.type) {
      case 'point': {
        group.add(makePoints([data.p], this.pointMat));
        labels.push(this.label(data.p, [
          { text: `X ${formatCoordinate(data.p.x, units)}`, cls: 'x' },
          { text: `Y ${formatCoordinate(data.p.y, units)}`, cls: 'y' },
          { text: `Z ${formatCoordinate(data.p.z, units)}`, cls: 'z' },
          { text: lengthUnit, cls: 'sub' },
        ], onDelete, 'offset'));
        break;
      }

      case 'distance': {
        const { p1, p2, extensions } = data.result;
        group.add(this.buildHighlight(data.from, false), this.buildHighlight(data.to, true));
        group.add(makeLines([...p1.toArray(), ...p2.toArray()], this.mat.main));
        group.add(makePoints([p1, p2], this.pointMat));
        if (extensions.length > 0) {
          group.add(makeLines(extensions.flatMap(([a, b]) => [...a.toArray(), ...b.toArray()]), this.mat.thin));
        }

        const distance = p1.distanceTo(p2);
        const prefix = data.perpendicular ? '⟂ ' : '';
        labels.push(this.label(p1.clone().lerp(p2, 0.5), [{ text: prefix + this.fmt('length', distance) }], onDelete));
        if (this.settings.showComponents && distance > 0) this.buildComponents(p1, p2, group, labels);
        break;
      }

      case 'angle': {
        if (data.from) group.add(this.buildHighlight(data.from, false));
        if (data.to) group.add(this.buildHighlight(data.to, true));
        const labelPos = this.buildArc(data.result, data.radius, group);
        labels.push(this.label(labelPos, [{ text: `∠ ${this.fmt('angle', data.result.angle)}` }], onDelete));
        break;
      }

      case 'area': {
        const { region } = data;
        group.add(makeMesh(region.triangles, this.fillMat));
        group.add(makeLines(region.outline, this.mat.outline));
        const rows = [{ text: this.fmt('area', region.area) }];
        // A closed surface (e.g. a sphere) has no outline
        if (region.perimeter > 0) {
          rows.push({ text: `${t('ribbon:measure.label.perimeter')} ${this.fmt('length', region.perimeter)}`, cls: 'sub' });
        }
        if (region.curved) rows.push({ text: t('ribbon:measure.label.curved'), cls: 'sub' });
        labels.push(this.label(region.centroid, rows, onDelete));
        break;
      }

      case 'polygon': {
        const { points, result } = data;
        const loop = points.flatMap((p, i) => [...p.toArray(), ...points[(i + 1) % points.length].toArray()]);
        const open = !placed && points.length <= 2;
        if (loop.length > 6 || !open) group.add(makeLines(open ? loop.slice(0, -6) : loop, this.mat.main));
        group.add(makePoints(points, this.pointMat));
        if (result.triangles.length > 0) {
          const tris = new Float32Array(result.triangles.flatMap((i) => points[i].toArray()));
          group.add(makeMesh(tris, this.fillMat));
        }
        if (points.length >= 3) {
          labels.push(this.label(result.centroid, [
            { text: this.fmt('area', result.area) },
            { text: `${t('ribbon:measure.label.perimeter')} ${this.fmt('length', result.perimeter)}`, cls: 'sub' },
          ], onDelete));
        }
        break;
      }

      case 'object': {
        const { result, object } = data;
        group.add(makeLines(data.outline, this.mat.outline));
        const rows: { text: string; cls?: string }[] = [];
        if (data.method === 'weight') {
          const w = this.weightOf(data);
          rows.push({ text: this.fmt('weight', w.mass) });
          if (object.material) rows.push({ text: object.material, cls: 'sub' });
          rows.push({ text: w.density !== undefined ? `V ${this.fmt('volume', result.volume)} · ${w.detail}` : w.detail, cls: 'sub' });
        } else if (data.method === 'areaTotal') {
          rows.push({ text: `${t('ribbon:measure.label.total')} ${this.fmt('area', result.surfaceArea)}` });
        } else {
          rows.push({ text: `V ${this.fmt('volume', result.volume)}` });
        }
        if (object.name) rows.push({ text: object.name, cls: 'sub' });
        if (!result.closed && data.method !== 'areaTotal' && !(data.method === 'weight' && object.fileWeight !== undefined)) {
          rows.push({ text: t('ribbon:measure.label.notClosed'), cls: 'warn' });
        }
        // Volume, weight and total area of one object share its centre; stack their labels
        labels.push(this.label(result.center, rows, onDelete, `stack-${data.method}`));
        break;
      }

      case 'count': {
        const counts = this.measurements.filter((m) => m.data.type === 'count');
        const index = counts.findIndex((m) => m.id === id);
        if (data.outline) group.add(makeLines(data.outline, this.mat.outline));
        if (data.faceTriangles) group.add(makeMesh(data.faceTriangles, this.hoverFillMat));
        if (data.edge) group.add(makeLines([...data.edge.a.toArray(), ...data.edge.b.toArray()], this.mat.hover));
        group.add(makePoints([data.p], this.pointMat));
        labels.push(this.label(data.p, [{ text: String(index < 0 ? counts.length + 1 : index + 1) }], onDelete, 'count'));
        break;
      }
    }

    return { group, labels };
  }

  /**
   * Arc with an arrow from the first leg (v1) to the second (v2), plus the
   * legs themselves. Returns where the label goes.
   */
  private buildArc(r: AngleResult, radius: number, group: THREE.Group): THREE.Vector3 {
    const { center, v1, v2, angle } = r;
    const legs = [
      ...center.toArray(), ...center.clone().addScaledVector(v1, Math.max(r.len1, radius * 1.3)).toArray(),
      ...center.toArray(), ...center.clone().addScaledVector(v2, Math.max(r.len2, radius * 1.3)).toArray(),
    ];
    group.add(makeLines(legs, this.mat.thin));
    group.add(makePoints([center], this.pointMat));

    let axis = new THREE.Vector3().crossVectors(v1, v2);
    if (axis.lengthSq() < 1e-12) {
      const ref = Math.abs(v1.z) < 0.9 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(1, 0, 0);
      axis = new THREE.Vector3().crossVectors(v1, ref);
    }
    axis.normalize();

    const steps = Math.max(8, Math.ceil(angle / (Math.PI / 48)));
    const arc: number[] = [];
    let prev = center.clone().addScaledVector(v1, radius);
    for (let i = 1; i <= steps; i++) {
      const p = center.clone().addScaledVector(v1.clone().applyAxisAngle(axis, angle * i / steps), radius);
      arc.push(...prev.toArray(), ...p.toArray());
      prev = p;
    }
    // Arrow head at the end of the arc, pointing along it
    if (angle > 1e-4) {
      const tip = prev;
      const end = v1.clone().applyAxisAngle(axis, angle);
      const tangent = new THREE.Vector3().crossVectors(axis, end).normalize();
      const h = radius * Math.min(0.25, Math.max(0.12, angle * 0.15));
      for (const side of [1, -1]) {
        const wing = tip.clone().addScaledVector(tangent, -h).addScaledVector(end, side * h * 0.55);
        arc.push(...tip.toArray(), ...wing.toArray());
      }
    }
    group.add(makeLines(arc, this.mat.main));

    const bisector = v1.clone().applyAxisAngle(axis, angle / 2);
    return center.clone().addScaledVector(bisector, radius * 1.45);
  }

  /** dX / dY / dZ legs of a distance: red, green and blue, as in Trimble Connect */
  private buildComponents(p1: THREE.Vector3, p2: THREE.Vector3, group: THREE.Group, labels: Label[]): void {
    const cx = new THREE.Vector3(p2.x, p1.y, p1.z);
    const cy = new THREE.Vector3(p2.x, p2.y, p1.z);
    const legs: [THREE.Vector3, THREE.Vector3, 'x' | 'y' | 'z', number][] = [
      [p1, cx, 'x', p2.x - p1.x],
      [cx, cy, 'y', p2.y - p1.y],
      [cy, p2, 'z', p2.z - p1.z],
    ];
    const total = p1.distanceTo(p2);
    for (const [a, b, axis, delta] of legs) {
      // Skip legs that are (almost) zero or the distance itself
      if (Math.abs(delta) < total * 1e-4 || Math.abs(Math.abs(delta) - total) < total * 1e-6) continue;
      group.add(makeLines([...a.toArray(), ...b.toArray()], this.mat[axis]));
      labels.push(this.label(a.clone().lerp(b, 0.5), [
        { text: `Δ${axis.toUpperCase()} ${this.fmt('length', Math.abs(delta))}` },
      ], undefined, `component ${axis}`));
    }
  }

  private label(
    pos: THREE.Vector3,
    rows: { text: string; cls?: string }[],
    onDelete?: () => void,
    variant = '',
  ): Label {
    const el = document.createElement('div');
    el.className = `measure-label ${variant}`.trim();
    const body = document.createElement('div');
    body.className = 'measure-label-body';
    for (const row of rows) {
      const line = document.createElement('div');
      if (row.cls) line.className = row.cls;
      line.textContent = row.text;
      body.appendChild(line);
    }
    el.appendChild(body);
    if (onDelete) {
      const del = document.createElement('button');
      del.className = 'measure-label-delete';
      del.textContent = '×';
      del.title = t('ribbon:measure.delete');
      del.addEventListener('pointerdown', (e) => e.stopPropagation());
      del.addEventListener('click', (e) => {
        e.stopPropagation();
        onDelete();
      });
      el.appendChild(del);
    }
    return { el, pos: pos.clone() };
  }
}

/** Plain highlight data from a snap (drops the scene object references) */
export function toHighlight(snap: SnapResult): Highlight {
  return {
    kind: snap.kind,
    point: snap.point.clone(),
    edge: snap.edge ? { a: snap.edge.a.clone(), b: snap.edge.b.clone() } : undefined,
    normal: snap.normal.clone(),
  };
}

// ============================================================================
// Helpers
// ============================================================================

function makeLines(positions: ArrayLike<number>, material: LineMaterial): LineSegments2 {
  const geometry = new LineSegmentsGeometry();
  geometry.setPositions(positions instanceof Float32Array ? positions : Array.from(positions));
  const lines = new LineSegments2(geometry, material);
  lines.renderOrder = 1001;
  lines.frustumCulled = false;
  return lines;
}

function makePoints(points: THREE.Vector3[], material: THREE.PointsMaterial): THREE.Points {
  const geometry = new THREE.BufferGeometry().setFromPoints(points);
  const p = new THREE.Points(geometry, material);
  p.renderOrder = 1002;
  p.frustumCulled = false;
  return p;
}

function makeMesh(triangles: Float32Array, material: THREE.Material): THREE.Mesh {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(triangles, 3));
  const mesh = new THREE.Mesh(geometry, material);
  mesh.renderOrder = 1000;
  mesh.frustumCulled = false;
  return mesh;
}

/** Unit square (side 1) lying in the plane with the given normal, as line segments */
function unitSquare(normal: THREE.Vector3): number[] {
  const ref = Math.abs(normal.z) < 0.99 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(1, 0, 0);
  const u = new THREE.Vector3().crossVectors(ref, normal).normalize().multiplyScalar(0.5);
  const v = new THREE.Vector3().crossVectors(normal, u);
  const c = [
    u.clone().add(v), u.clone().sub(v), u.clone().negate().sub(v), u.clone().negate().add(v),
  ];
  return c.flatMap((p, i) => [...p.toArray(), ...c[(i + 1) % 4].toArray()]);
}

/** Dispose geometries of all children (materials are shared and kept) */
function disposeChildren(group: THREE.Group): void {
  for (const child of [...group.children]) {
    child.traverse((o) => {
      if (o instanceof THREE.Mesh || o instanceof THREE.Points || o instanceof LineSegments2) o.geometry.dispose();
    });
    group.remove(child);
  }
}

function disposeGroup(group: THREE.Group): void {
  disposeChildren(group);
  group.parent?.remove(group);
}
