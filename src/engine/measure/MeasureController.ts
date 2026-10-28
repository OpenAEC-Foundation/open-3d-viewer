import * as THREE from 'three';
import { worldPerPixel } from '../sectionplane/SectionPlaneGizmo';
import { MeasureManager, toHighlight, type MeasurementData, type ObjectDescription } from './MeasureManager';
import { MeasureSnapper, snapToEntity, type SnapResult, type SnapSettings } from './MeasureSnapper';
import {
  angleBetween, angleThreePoints, flatFaceRegion, objectVolume, perpendicularDistance, polygonArea,
  type AngleResult, type FaceRegion,
} from './measureMath';
import { objectFor, type ObjectInfo } from './measureObjects';
import type { MeasureTool } from '../../state/slices/measureSlice';

/** Clicks that move further than this (pixels) are orbit gestures */
const CLICK_TOLERANCE = 5;
/** Clicking this close (pixels) to the first polygon point closes the polygon */
const CLOSE_PIXELS = 10;
/** Radius of angle arcs in screen pixels (at the moment they are placed) */
const ARC_PIXELS = 55;

/**
 * Pointer interaction for the measure tools.
 *
 * Length:  distance (point to point), perpendicular (point / edge / face), point (coordinates)
 * Area:    area (one face, flat or curved), polygon (clicked corner points), areaTotal (all faces of an object)
 * Object:  volume, weight
 * Angle:   angle (between edges and/or faces), angle3 (three points)
 * Count:   count (objects), countEntity (points, edges, faces)
 *
 * Must be created before the selection handler, because it stops the clicks it
 * consumes from reaching it. Orbiting keeps working while measuring.
 */
export class MeasureController {
  private tool: MeasureTool = 'off';
  private snapSettings: SnapSettings = { vertex: true, midpoint: true, edge: true, face: true };

  private camera: THREE.Camera;
  private canvas: HTMLCanvasElement;
  private manager: MeasureManager;
  private snapper: MeasureSnapper;
  private onStageChange?: (stage: number) => void;
  private onExit?: () => void;

  private ndc = new THREE.Vector2();
  private px = new THREE.Vector2();
  private size = new THREE.Vector2();
  private downPos = { x: 0, y: 0 };
  private hoverFrame = 0;

  /** First pick of a two-pick measurement */
  private first: SnapResult | null = null;
  /** Picked points of a polygon or a three-point angle */
  private points: THREE.Vector3[] = [];
  /** Last hovered face (reused while the cursor stays on it) */
  private hoverRegion: { mesh: THREE.Mesh; instanceId?: number; region: FaceRegion } | null = null;
  private hoverObjectKey = '';

  constructor(
    manager: MeasureManager,
    camera: THREE.Camera,
    canvas: HTMLCanvasElement,
    options: {
      getPickTargets: () => THREE.Object3D[];
      getClippingPlanes: () => THREE.Plane[];
      /** Number of picks made in the current measurement */
      onStageChange?: (stage: number) => void;
      /** Escape pressed with nothing pending */
      onExit?: () => void;
    },
  ) {
    this.manager = manager;
    this.camera = camera;
    this.canvas = canvas;
    this.snapper = new MeasureSnapper(options.getPickTargets, options.getClippingPlanes);
    this.onStageChange = options.onStageChange;
    this.onExit = options.onExit;

    this.handlePointerMove = this.handlePointerMove.bind(this);
    this.handlePointerDown = this.handlePointerDown.bind(this);
    this.handlePointerLeave = this.handlePointerLeave.bind(this);
    this.handleClick = this.handleClick.bind(this);
    this.handleDoubleClick = this.handleDoubleClick.bind(this);
    this.handleKeyDown = this.handleKeyDown.bind(this);

    canvas.addEventListener('pointermove', this.handlePointerMove);
    canvas.addEventListener('pointerdown', this.handlePointerDown);
    canvas.addEventListener('pointerleave', this.handlePointerLeave);
    canvas.addEventListener('click', this.handleClick);
    canvas.addEventListener('dblclick', this.handleDoubleClick);
    window.addEventListener('keydown', this.handleKeyDown);
  }

  setCamera(camera: THREE.Camera): void {
    this.camera = camera;
  }

  setTool(tool: MeasureTool): void {
    if (tool === this.tool) return;
    this.reset();
    this.tool = tool;
    this.canvas.style.cursor = tool === 'off' ? '' : 'crosshair';
  }

  setSnapSettings(settings: SnapSettings): void {
    this.snapSettings = settings;
  }

  /** Cancel the measurement in progress */
  reset(): void {
    this.first = null;
    this.points = [];
    this.hoverRegion = null;
    this.hoverObjectKey = '';
    this.manager.clearTransient();
    this.onStageChange?.(0);
  }

  dispose(): void {
    cancelAnimationFrame(this.hoverFrame);
    this.canvas.removeEventListener('pointermove', this.handlePointerMove);
    this.canvas.removeEventListener('pointerdown', this.handlePointerDown);
    this.canvas.removeEventListener('pointerleave', this.handlePointerLeave);
    this.canvas.removeEventListener('click', this.handleClick);
    this.canvas.removeEventListener('dblclick', this.handleDoubleClick);
    window.removeEventListener('keydown', this.handleKeyDown);
  }

  // ============================================================================
  // Event handlers
  // ============================================================================

  private updateMouse(e: MouseEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    this.size.set(rect.width, rect.height);
    this.px.set(e.clientX - rect.left, e.clientY - rect.top);
    this.ndc.set((this.px.x / rect.width) * 2 - 1, -(this.px.y / rect.height) * 2 + 1);
  }

  private handlePointerMove(e: PointerEvent): void {
    if (this.tool === 'off') return;
    this.canvas.style.cursor = 'crosshair';
    if (e.buttons !== 0) return; // orbiting
    this.updateMouse(e);
    if (!this.hoverFrame) {
      this.hoverFrame = requestAnimationFrame(() => {
        this.hoverFrame = 0;
        this.updateHover();
      });
    }
  }

  private handlePointerDown(e: PointerEvent): void {
    this.downPos = { x: e.clientX, y: e.clientY };
  }

  private handlePointerLeave(): void {
    if (this.tool === 'off') return;
    this.manager.clearHover();
    if (this.first) this.manager.setPreview(null);
    if (this.tool === 'polygon') this.updatePolygonPreview(null);
    if (this.tool === 'angle3') this.updateAngle3Preview(null);
    this.hoverObjectKey = '';
  }

  private handleClick(e: MouseEvent): void {
    if (this.tool === 'off') return;
    // While measuring, clicks never select objects
    e.stopImmediatePropagation();
    if (Math.abs(e.clientX - this.downPos.x) > CLICK_TOLERANCE || Math.abs(e.clientY - this.downPos.y) > CLICK_TOLERANCE) return;

    this.updateMouse(e);
    switch (this.tool) {
      case 'point': {
        const snap = this.snap();
        if (snap) this.manager.add({ type: 'point', p: snap.point });
        break;
      }

      case 'distance':
      case 'perpendicular':
      case 'angle': {
        const snap = this.snap();
        if (!snap) break;
        if (!this.first) {
          this.first = snap;
          this.manager.setPending(toHighlight(snap));
          this.onStageChange?.(1);
        } else {
          this.manager.add(this.twoPickData(this.first, snap));
          this.first = null;
          this.manager.setPending(null);
          this.manager.setPreview(null);
          this.onStageChange?.(0);
        }
        break;
      }

      case 'angle3': {
        const snap = this.snap();
        if (!snap) break;
        this.points.push(snap.point);
        if (this.points.length === 3) {
          const result = angleThreePoints(this.points[0], this.points[1], this.points[2]);
          if (result) this.manager.add({ type: 'angle', result, radius: this.arcRadius(result.center), three: true });
          this.points = [];
          this.manager.setPreview(null);
        } else {
          this.updateAngle3Preview(null);
        }
        this.onStageChange?.(this.points.length);
        break;
      }

      case 'polygon': {
        const snap = this.snap();
        if (!snap) break;
        if (this.points.length >= 3 && this.isNearFirstPoint()) {
          this.finishPolygon();
        } else {
          this.points.push(snap.point);
          this.onStageChange?.(this.points.length);
          this.updatePolygonPreview(null);
        }
        break;
      }

      case 'area': {
        const hit = this.snapper.pick(this.ndc, this.camera);
        if (!hit) break;
        this.manager.add({ type: 'area', region: this.regionFor(hit), object: describe(objectFor(hit.object, hit.instanceId)) });
        break;
      }

      case 'areaTotal':
      case 'volume':
      case 'weight': {
        const hit = this.snapper.pick(this.ndc, this.camera);
        if (!hit) break;
        const object = objectFor(hit.object, hit.instanceId);
        this.manager.add({
          type: 'object',
          method: this.tool,
          result: objectVolume(object.parts),
          outline: this.manager.objectOutline(object.parts),
          object: describe(object),
        });
        break;
      }

      case 'count': {
        const hit = this.snapper.pick(this.ndc, this.camera);
        if (!hit) break;
        const object = objectFor(hit.object, hit.instanceId);
        this.toggleCount({
          type: 'count', method: 'count', key: object.key, p: hit.point.clone(),
          outline: this.manager.objectOutline(object.parts),
          object: describe(object),
        });
        break;
      }

      case 'countEntity': {
        const snap = this.snap();
        if (snap) this.toggleCount(this.entityCountData(snap));
        break;
      }
    }
    this.updateHover();
  }

  private handleDoubleClick(e: MouseEvent): void {
    if (this.tool !== 'polygon') return;
    e.stopImmediatePropagation();
    // The second click of the double click added a duplicate point
    const n = this.points.length;
    if (n >= 2 && this.points[n - 1].distanceTo(this.points[n - 2]) < 1e-9) this.points.pop();
    this.finishPolygon();
  }

  private handleKeyDown(e: KeyboardEvent): void {
    if (this.tool === 'off') return;
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'SELECT' || target.tagName === 'TEXTAREA')) return;

    if (e.key === 'Escape') {
      if (this.first || this.points.length > 0) this.reset();
      else this.onExit?.();
    } else if (e.key === 'Enter' && this.tool === 'polygon') {
      this.finishPolygon();
    } else if (e.key === 'Backspace' && (this.tool === 'polygon' || this.tool === 'angle3') && this.points.length > 0) {
      e.preventDefault();
      this.points.pop();
      this.onStageChange?.(this.points.length);
      if (this.tool === 'polygon') this.updatePolygonPreview(null);
      else this.updateAngle3Preview(null);
    }
  }

  // ============================================================================
  // Hover & helpers
  // ============================================================================

  /** Snap settings for the current tool (angles need edges or faces) */
  private snap(): SnapResult | null {
    const s = this.snapSettings;
    const settings = this.tool === 'angle'
      ? { vertex: false, midpoint: false, edge: s.edge, face: s.face }
      : s;
    return this.snapper.snap(this.ndc, this.px, this.size, this.camera, settings);
  }

  private updateHover(): void {
    switch (this.tool) {
      case 'off':
        return;

      case 'point':
      case 'distance':
      case 'perpendicular':
      case 'angle':
      case 'countEntity': {
        const snap = this.snap();
        this.manager.showSnap(snap, !!this.first);
        if (this.first) this.manager.setPreview(snap ? this.twoPickData(this.first, snap) : null);
        return;
      }

      case 'polygon':
      case 'angle3': {
        const snap = this.snap();
        this.manager.showSnap(snap);
        if (this.tool === 'polygon') this.updatePolygonPreview(snap?.point ?? null);
        else this.updateAngle3Preview(snap?.point ?? null);
        return;
      }

      case 'area': {
        const hit = this.snapper.pick(this.ndc, this.camera);
        this.manager.showFaceHover(hit ? this.regionFor(hit) : null);
        return;
      }

      case 'areaTotal':
      case 'volume':
      case 'weight':
      case 'count': {
        const hit = this.snapper.pick(this.ndc, this.camera);
        const object = hit ? objectFor(hit.object, hit.instanceId) : null;
        const key = object?.key ?? '';
        if (key === this.hoverObjectKey) return;
        this.hoverObjectKey = key;
        this.manager.showObjectHover(object ? this.manager.objectOutline(object.parts) : null);
        return;
      }
    }
  }

  /** Distance, perpendicular distance or angle between two picks */
  private twoPickData(a: SnapResult, b: SnapResult): MeasurementData {
    const from = toHighlight(a);
    const to = toHighlight(b);
    if (this.tool === 'angle') {
      const result = angleBetween(snapToEntity(a), snapToEntity(b)) ?? parallelAngle(a.point, b.point);
      return { type: 'angle', result, radius: this.arcRadius(result.center), three: false, from, to };
    }
    if (this.tool === 'perpendicular') {
      return { type: 'distance', result: perpendicularDistance(snapToEntity(a), snapToEntity(b)), perpendicular: true, from, to };
    }
    return {
      type: 'distance',
      result: { p1: a.point.clone(), p2: b.point.clone(), extensions: [] },
      perpendicular: false, from, to,
    };
  }

  private arcRadius(center: THREE.Vector3): number {
    return worldPerPixel(this.camera, center, this.size.y || 1) * ARC_PIXELS;
  }

  /** Counted point, edge or face for a snap */
  private entityCountData(snap: SnapResult): Extract<MeasurementData, { type: 'count' }> {
    const r = (v: THREE.Vector3) => v.toArray().map((c) => Math.round(c * 1e4)).join(',');
    if (snap.kind === 'face') {
      const region = flatFaceRegion(snap.object, snap.faceIndex, snap.instanceId);
      const first = Math.min(...region.faces);
      return {
        type: 'count', method: 'countEntity', kind: 'face', p: snap.point.clone(),
        key: `f:${snap.object.uuid}:${snap.instanceId ?? ''}:${first}`,
        faceTriangles: region.triangles,
      };
    }
    if (snap.kind === 'edge' && snap.edge) {
      const [a, b] = [r(snap.edge.a), r(snap.edge.b)].sort();
      const mid = snap.edge.a.clone().add(snap.edge.b).multiplyScalar(0.5);
      return {
        type: 'count', method: 'countEntity', kind: 'edge', p: mid,
        key: `e:${a}|${b}`, edge: { a: snap.edge.a.clone(), b: snap.edge.b.clone() },
      };
    }
    return { type: 'count', method: 'countEntity', kind: 'vertex', p: snap.point.clone(), key: `p:${r(snap.point)}` };
  }

  /** Clicking a counted item again removes it */
  private toggleCount(data: Extract<MeasurementData, { type: 'count' }>): void {
    const existing = this.manager.findCount(data.key);
    if (existing !== null) this.manager.remove(existing);
    else this.manager.add(data);
  }

  /** Face under a hit, reusing the last one while the cursor stays on it */
  private regionFor(hit: THREE.Intersection<THREE.Mesh>): FaceRegion {
    const cached = this.hoverRegion;
    if (cached && cached.mesh === hit.object && cached.instanceId === hit.instanceId && cached.region.faces.has(hit.faceIndex!)) {
      return cached.region;
    }
    const region = flatFaceRegion(hit.object, hit.faceIndex!, hit.instanceId);
    this.hoverRegion = { mesh: hit.object, instanceId: hit.instanceId, region };
    return region;
  }

  private isNearFirstPoint(): boolean {
    const ndc = this.points[0].clone().project(this.camera);
    const x = (ndc.x + 1) / 2 * this.size.x;
    const y = (1 - ndc.y) / 2 * this.size.y;
    return Math.hypot(x - this.px.x, y - this.px.y) < CLOSE_PIXELS;
  }

  private updatePolygonPreview(cursor: THREE.Vector3 | null): void {
    const points = cursor ? [...this.points, cursor] : [...this.points];
    if (points.length === 0) {
      this.manager.setPreview(null);
      return;
    }
    this.manager.setPreview({ type: 'polygon', points, result: polygonArea(points) });
  }

  private updateAngle3Preview(cursor: THREE.Vector3 | null): void {
    const points = cursor ? [...this.points, cursor] : [...this.points];
    if (points.length === 3) {
      const result = angleThreePoints(points[0], points[1], points[2]);
      if (result) {
        this.manager.setPreview({ type: 'angle', result, radius: this.arcRadius(result.center), three: true });
        return;
      }
    }
    // One or two points: show the legs as a polygon outline
    this.manager.setPreview(points.length > 0 ? { type: 'polygon', points, result: polygonArea([]) } : null);
  }

  private finishPolygon(): void {
    if (this.points.length >= 3) {
      const points = this.points;
      this.manager.add({ type: 'polygon', points, result: polygonArea(points) });
    }
    this.points = [];
    this.manager.setPreview(null);
    this.onStageChange?.(0);
  }
}

function describe(object: ObjectInfo): ObjectDescription {
  return { name: object.name, material: object.material, fileWeight: object.weight };
}

/** 0° result for parallel edges / faces */
function parallelAngle(a: THREE.Vector3, b: THREE.Vector3): AngleResult {
  const dir = b.clone().sub(a);
  const len = dir.length();
  if (len > 0) dir.divideScalar(len); else dir.set(1, 0, 0);
  return { angle: 0, center: a.clone(), v1: dir.clone(), v2: dir, len1: len, len2: len };
}
