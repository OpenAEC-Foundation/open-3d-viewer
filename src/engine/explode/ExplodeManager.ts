import * as THREE from 'three';
import type { ExplodeMode } from '../../state/slices/viewerSlice';

/** One mesh of an exploded unit, with its rest position */
interface ExplodePart {
  object: THREE.Object3D;
  rest: THREE.Vector3;
  /** World → parent-local direction transform (linear part of the parent's inverse world matrix) */
  toLocal: THREE.Matrix3;
}

/** Everything that moves together: one IFC element (all its meshes) or one mesh */
interface ExplodeUnit {
  parts: ExplodePart[];
  /** World-space bounds at rest */
  box: THREE.Box3;
  /** World-space centre at rest */
  center: THREE.Vector3;
  /** IFC storey expressID while building, then replaced by `storey` */
  storeyId?: number;
  /** Storey index (0 = lowest), -1 when the element has no storey */
  storey: number;
}

/** At 100 %, elements move to 3× their distance from the scene centre */
const ELEMENT_SPREAD = 2;
/** At 100 %, storeys are laid out side by side with a 10 % gap */
const STOREY_SPREAD_XY = 1.1;
/** At 100 %, each storey gap grows by two storey heights */
const STOREY_SPREAD_Z = 2;

/**
 * Exploded view: pulls the loaded models apart along X, Y and Z, either per
 * element (away from the scene centre) or per building storey (IFC).
 * Only positions of meshes change; geometry and the scene graph stay intact,
 * so an amount of 0 on every axis restores the model exactly.
 */
export class ExplodeManager {
  private units: ExplodeUnit[] = [];
  private center = new THREE.Vector3();
  private size = new THREE.Vector3();
  /** Size of the elements that belong to a storey (the building without its site) */
  private storeySize = new THREE.Vector3();
  private storeyCount = 0;
  private storeyHeight = 0;
  private mode: ExplodeMode = 'storeys';
  /** Amount per axis, 0..1 */
  private amount = new THREE.Vector3();

  constructor(
    private getModelGroups: () => THREE.Group[],
    private onStoreyCountChange?: (count: number) => void,
  ) {}

  /** Call when models are added or removed. */
  invalidate(): void {
    this.restore();
    this.build();
    this.apply();
  }

  /** Set the mode and the amount per axis (0..1) and move the elements. */
  set(mode: ExplodeMode, x: number, y: number, z: number): void {
    this.mode = mode;
    this.amount.set(x, y, z);
    this.apply();
  }

  /** Storeys mode needs storey data; otherwise elements are exploded */
  get effectiveMode(): ExplodeMode {
    return this.mode === 'storeys' && this.storeyCount > 0 ? 'storeys' : 'elements';
  }

  private build(): void {
    this.units = [];
    const bounds = new THREE.Box3();
    let storeyCount = 0;

    for (const group of this.getModelGroups()) {
      group.updateMatrixWorld(true);
      const modelUnits: ExplodeUnit[] = [];
      const unitOfMesh = new Map<THREE.Object3D, ExplodeUnit>();
      // IFC elements can consist of several meshes (frame + glass); move them together
      const unitOfElement = new Map<number, ExplodeUnit>();

      group.traverse((obj) => {
        if (!(obj instanceof THREE.Mesh) || !obj.geometry) return;
        const geo = obj.geometry as THREE.BufferGeometry;
        if (!geo.boundingBox) geo.computeBoundingBox();
        if (!geo.boundingBox || geo.boundingBox.isEmpty()) return;
        const box = geo.boundingBox.clone().applyMatrix4(obj.matrixWorld);
        bounds.union(box);

        // A mesh inside another mesh already moves with it (traverse visits parents first)
        let owner: THREE.Object3D | null = obj.parent;
        while (owner && owner !== group && !unitOfMesh.has(owner)) owner = owner.parent;
        const nested = owner && owner !== group ? unitOfMesh.get(owner) : undefined;
        if (nested) {
          nested.box.union(box);
          unitOfMesh.set(obj, nested);
          return;
        }

        const part: ExplodePart = {
          object: obj,
          rest: obj.position.clone(),
          toLocal: new THREE.Matrix3().setFromMatrix4(
            obj.parent ? obj.parent.matrixWorld.clone().invert() : new THREE.Matrix4(),
          ),
        };
        const expressID = obj.userData?.expressID;
        let unit = typeof expressID === 'number' ? unitOfElement.get(expressID) : undefined;
        if (unit) {
          unit.parts.push(part);
          unit.box.union(box);
        } else {
          const storeyId = obj.userData?.storeyId;
          unit = {
            parts: [part], box, center: new THREE.Vector3(), storey: -1,
            storeyId: typeof storeyId === 'number' ? storeyId : undefined,
          };
          if (typeof expressID === 'number') unitOfElement.set(expressID, unit);
          modelUnits.push(unit);
        }
        unitOfMesh.set(obj, unit);
      });

      // Order the model's storeys low → high: by stated elevation when every storey
      // has one, else by the average base of their elements
      const elevation = new Map<number, number>();
      const baseZ = new Map<number, { sum: number; n: number }>();
      group.traverse((obj) => {
        const id = obj.userData?.storeyId;
        const value = obj.userData?.storeyElevation;
        if (typeof id === 'number' && typeof value === 'number' && Number.isFinite(value)) elevation.set(id, value);
      });
      for (const unit of modelUnits) {
        unit.box.getCenter(unit.center);
        if (unit.storeyId === undefined) continue;
        const z = baseZ.get(unit.storeyId) ?? { sum: 0, n: 0 };
        z.sum += unit.box.min.z;
        z.n++;
        baseZ.set(unit.storeyId, z);
      }
      const ids = [...baseZ.keys()];
      const byElevation = ids.every((id) => elevation.has(id));
      const key = (id: number) => byElevation ? elevation.get(id)! : baseZ.get(id)!.sum / baseZ.get(id)!.n;
      ids.sort((a, b) => key(a) - key(b));
      const index = new Map(ids.map((id, i) => [id, i]));
      for (const unit of modelUnits) {
        if (unit.storeyId !== undefined) unit.storey = index.get(unit.storeyId) ?? -1;
      }

      storeyCount = Math.max(storeyCount, ids.length);
      this.units.push(...modelUnits);
    }

    if (bounds.isEmpty()) {
      this.center.set(0, 0, 0);
      this.size.set(0, 0, 0);
    } else {
      bounds.getCenter(this.center);
      bounds.getSize(this.size);
    }
    const storeyBounds = new THREE.Box3();
    for (const unit of this.units) {
      if (unit.storey >= 0) storeyBounds.union(unit.box);
    }
    if (storeyBounds.isEmpty()) this.storeySize.set(0, 0, 0);
    else storeyBounds.getSize(this.storeySize);
    this.storeyHeight = storeyCount > 0 ? this.storeySize.z / storeyCount : 0;
    if (storeyCount !== this.storeyCount) {
      this.storeyCount = storeyCount;
      this.onStoreyCountChange?.(storeyCount);
    }
  }

  private apply(): void {
    const { x: ax, y: ay, z: az } = this.amount;
    if (ax === 0 && ay === 0 && az === 0) {
      this.restore();
      return;
    }

    const storeys = this.effectiveMode === 'storeys';
    const offset = new THREE.Vector3();
    const local = new THREE.Vector3();
    for (const unit of this.units) {
      if (!storeys) {
        offset.subVectors(unit.center, this.center).multiply(this.amount).multiplyScalar(ELEMENT_SPREAD);
      } else if (unit.storey < 0) {
        offset.set(0, 0, 0);
      } else {
        offset.set(
          unit.storey * ax * STOREY_SPREAD_XY * this.storeySize.x,
          unit.storey * ay * STOREY_SPREAD_XY * this.storeySize.y,
          unit.storey * az * STOREY_SPREAD_Z * this.storeyHeight,
        );
      }
      for (const part of unit.parts) {
        local.copy(offset).applyMatrix3(part.toLocal);
        part.object.position.copy(part.rest).add(local);
      }
    }
  }

  private restore(): void {
    for (const unit of this.units) {
      for (const part of unit.parts) part.object.position.copy(part.rest);
    }
  }
}
