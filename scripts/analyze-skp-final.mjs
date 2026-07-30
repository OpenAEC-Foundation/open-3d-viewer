/**
 * FINAL analysis: Map de_05 entity IDs from instances to 8e_13 child IDs in definitions.
 * This should resolve all 188 unnamed instances.
 */

import { readFileSync } from 'fs';
import { inflateRawSync, inflateSync } from 'zlib';

const filePath = process.argv[2];
if (!filePath) { process.exit(1); }
const raw = readFileSync(filePath);

let zipStart = -1;
for (let i = 0; i < 500; i++) {
  if (raw[i]===0x50&&raw[i+1]===0x4b&&raw[i+2]===0x03&&raw[i+3]===0x04) { zipStart = i; break; }
}
const entries = [];
let pos = zipStart;
while (pos < raw.length-30) {
  if (raw[pos]!==0x50||raw[pos+1]!==0x4b||raw[pos+2]!==0x03||raw[pos+3]!==0x04) break;
  const cm = raw[pos+8]|(raw[pos+9]<<8);
  const cs = raw[pos+18]|(raw[pos+19]<<8)|(raw[pos+20]<<16)|(raw[pos+21]<<24);
  const nl = raw[pos+26]|(raw[pos+27]<<8);
  const el = raw[pos+28]|(raw[pos+29]<<8);
  const nm = raw.slice(pos+30, pos+30+nl).toString('utf-8');
  const ds = pos+30+nl+el;
  entries.push({ name:nm, comprMethod:cm, compSize:cs, dataStart:ds });
  pos = ds+cs;
}
const me = entries.find(e=>e.name==='model.dat')||entries.reduce((b,e)=>(!b||e.compSize>b.compSize)?e:b,null);
let md;
try { md = inflateSync(raw.slice(me.dataStart, me.dataStart+me.compSize)); }
catch { md = inflateRawSync(raw.slice(me.dataStart, me.dataStart+me.compSize)); }
const dv = new DataView(md.buffer, md.byteOffset, md.byteLength);

// Detect entity ID size (from de_05 tags)
let idSize = 3; // default

// ======================================================================
// Step 1: Find all definitions and their child entity IDs
// ======================================================================
const definitions = []; // { name, offset, 8e_13_children, nearby_entity_ids }

for (let i = 0; i < md.length - 10; i++) {
  if (md[i]===0x7e && md[i+1]===0x15) {
    const len = md[i+2]|(md[i+3]<<8)|(md[i+4]<<16)|(md[i+5]<<24);
    if (len <= 0 || len >= 300 || i+6+len > md.length) continue;
    const name = Buffer.from(md.slice(i+6, i+6+len)).toString('utf-8').replace(/\0/g,'').trim();
    if (!name) continue;

    // Scan backward from 7e_15 to find 8e_13, 67_19, 9e_13
    const children = new Set();
    const nearbyIds = new Set();

    for (let j = i - 1; j >= Math.max(0, i - 300); j--) {
      // Look for 8e_13 (child entity IDs)
      if (md[j]===0x8e && md[j+1]===0x13) {
        const l = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (l > 0 && l < 500 && j+6+l <= md.length) {
          // Parse entity IDs: prefix byte (03=3byte, 02=2byte, 01=1byte) + ID bytes
          let p = 0;
          const data = md.slice(j+6, j+6+l);
          while (p < data.length) {
            const prefix = data[p];
            const sz = prefix; // prefix IS the size
            if (sz >= 1 && sz <= 4 && p + 1 + sz <= data.length) {
              const eid = Array.from(data.slice(p+1, p+1+sz)).map(b=>b.toString(16).padStart(2,'0')).join('');
              children.add(eid);
              p += 1 + sz;
            } else {
              break;
            }
          }
        }
      }

      // Look for 67_19 (entity ID near definition)
      if (md[j]===0x67 && md[j+1]===0x19) {
        const l = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (l > 0 && l <= 8 && j+6+l <= md.length) {
          const eid = Array.from(md.slice(j+6, j+6+l)).map(b=>b.toString(16).padStart(2,'0')).join('');
          nearbyIds.add(eid);
        }
      }

      // Stop at 66_19 or 7c_15 (left the definition header region)
      if ((md[j]===0x66 && md[j+1]===0x19) || (md[j]===0x7c && md[j+1]===0x15)) break;
    }

    // Also scan FORWARD from 7e_15 for additional entity IDs in the definition data
    for (let j = i + 6 + len; j < Math.min(i + 6 + len + 400, md.length - 6); j++) {
      // 81_15 often has a 4-byte timestamp, skip
      // 85_15 is the main data block
      if (md[j]===0x85 && md[j+1]===0x15) {
        const l = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (l > 0 && l < 10000 && j+6+l <= md.length) {
          // Parse 85_15 data for de_05 entity IDs
          const data85 = md.slice(j+6, j+6+l);
          for (let k = 0; k < data85.length - 6; k++) {
            if (data85[k]===0xde && data85[k+1]===0x05) {
              const el = data85[k+2]|(data85[k+3]<<8)|(data85[k+4]<<16)|(data85[k+5]<<24);
              if (el >= 1 && el <= 4 && k+6+el <= data85.length) {
                const eid = Array.from(data85.slice(k+6, k+6+el)).map(b=>b.toString(16).padStart(2,'0')).join('');
                nearbyIds.add(eid);
                idSize = el; // detect entity ID size
              }
            }
          }
        }
        break;
      }
    }

    definitions.push({ name, offset: i, children: [...children], nearbyIds: [...nearbyIds] });
  }
}

console.log(`Definitions found: ${definitions.length}`);
console.log(`Entity ID size: ${idSize} bytes`);

// Build reverse map: child entity ID → definition name
const childIdToDefName = new Map();
for (const def of definitions) {
  for (const childId of def.children) {
    childIdToDefName.set(childId, def.name);
  }
  for (const eid of def.nearbyIds) {
    if (!childIdToDefName.has(eid)) {
      childIdToDefName.set(eid, def.name);
    }
  }
}
console.log(`Child/nearby ID → definition mappings: ${childIdToDefName.size}`);

// ======================================================================
// Step 2: Extract de_05 from each instance's 64_19/4c_1d data
// ======================================================================
const instances = [];
for (let i = 0; i < md.length - 110; i++) {
  if (md[i]===0x66 && md[i+1]===0x19) {
    const len = md[i+2]|(md[i+3]<<8)|(md[i+4]<<16)|(md[i+5]<<24);
    if (len !== 104) continue;
    const vals = [];
    let valid = true;
    for (let k = 0; k < 13; k++) {
      const v = dv.getFloat64(i+6+k*8, true);
      if (!isFinite(v)) { valid = false; break; }
      vals.push(v);
    }
    if (!valid || Math.abs(vals[12]-1.0) > 0.01) continue;

    let scanPos = i + 110;
    // 67_19
    let entityId = null;
    if (scanPos+6<=md.length && md[scanPos]===0x67&&md[scanPos+1]===0x19) {
      const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
      entityId = Array.from(md.slice(scanPos+6, scanPos+6+l)).map(b=>b.toString(16).padStart(2,'0')).join('');
      scanPos += 6+l;
    }
    // 68_19
    if (scanPos+6<=md.length && md[scanPos]===0x68&&md[scanPos+1]===0x19) {
      const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
      scanPos += 6+l;
    }

    // Check next tag
    let isDefHeader = false;
    let defName = null;
    let de05id = null;

    if (scanPos+2<=md.length && md[scanPos]===0x9e && md[scanPos+1]===0x13) {
      isDefHeader = true;
      // Find 7e_15
      for (let j = scanPos; j < Math.min(scanPos+300, md.length-10); j++) {
        if (md[j]===0x7e && md[j+1]===0x15) {
          const nlen = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
          if (nlen > 0 && nlen < 300 && j+6+nlen <= md.length) {
            defName = Buffer.from(md.slice(j+6, j+6+nlen)).toString('utf-8').replace(/\0/g,'').trim();
            break;
          }
        }
      }
    } else {
      // Reference instance — extract de_05 from the data
      // Scan forward looking for de_05 tag
      const searchEnd = Math.min(scanPos + 600, md.length - 6);
      for (let j = scanPos; j < searchEnd; j++) {
        if (md[j]===0xde && md[j+1]===0x05) {
          const l = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
          if (l >= 1 && l <= 4 && j+6+l <= md.length) {
            de05id = Array.from(md.slice(j+6, j+6+l)).map(b=>b.toString(16).padStart(2,'0')).join('');
            break;
          }
        }
        // Stop at next 66_19
        if (md[j]===0x66 && md[j+1]===0x19) {
          const nl = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
          if (nl === 104) break;
        }
      }
    }

    instances.push({ offset: i, entityId, isDefHeader, defName, de05id, vals });
  }
}

// ======================================================================
// Step 3: Match instances to definitions
// ======================================================================
// Strategy 1: Definition-header instances → use defName directly
// Strategy 2: Entity ID matching → use known entity ID → name mapping
// Strategy 3: de_05 → child entity ID → definition name

const eidToName = new Map();
for (const inst of instances) {
  if (inst.isDefHeader && inst.entityId && inst.defName) {
    eidToName.set(inst.entityId, inst.defName);
  }
}

let resolvedHeader = 0, resolvedEid = 0, resolvedDe05 = 0, unresolved = 0;

for (const inst of instances) {
  if (inst.isDefHeader) {
    resolvedHeader++;
    continue;
  }

  // Try entity ID mapping
  if (inst.entityId && eidToName.has(inst.entityId)) {
    inst.resolvedName = eidToName.get(inst.entityId);
    inst.resolvedVia = 'entity_id';
    resolvedEid++;
    continue;
  }

  // Try de_05 → child ID → definition mapping
  if (inst.de05id && childIdToDefName.has(inst.de05id)) {
    inst.resolvedName = childIdToDefName.get(inst.de05id);
    inst.resolvedVia = 'de_05→child_id';
    resolvedDe05++;
    continue;
  }

  unresolved++;
  inst.resolvedName = null;
}

console.log(`\n=== RESOLUTION RESULTS ===`);
console.log(`Definition-header instances: ${resolvedHeader}`);
console.log(`Resolved via entity ID: ${resolvedEid}`);
console.log(`Resolved via de_05 → child ID: ${resolvedDe05}`);
console.log(`Unresolved: ${unresolved}`);
console.log(`Total: ${instances.length}`);

// Show instances resolved via de_05
console.log(`\n=== de_05 RESOLVED INSTANCES (sample) ===`);
const de05resolved = instances.filter(i => i.resolvedVia === 'de_05→child_id');
for (const inst of de05resolved.slice(0, 20)) {
  const txM = (inst.vals[9] * 0.0254).toFixed(3);
  const tyM = (inst.vals[10] * 0.0254).toFixed(3);
  const tzM = (inst.vals[11] * 0.0254).toFixed(3);
  console.log(`  eid=${inst.entityId} de05=${inst.de05id} → "${inst.resolvedName}" translate=(${txM}, ${tyM}, ${tzM})m`);
}

// Show unresolved
console.log(`\n=== UNRESOLVED INSTANCES ===`);
const unresolvedInst = instances.filter(i => !i.isDefHeader && !i.resolvedName);
for (const inst of unresolvedInst.slice(0, 20)) {
  const txM = (inst.vals[9] * 0.0254).toFixed(3);
  const tyM = (inst.vals[10] * 0.0254).toFixed(3);
  const tzM = (inst.vals[11] * 0.0254).toFixed(3);
  console.log(`  eid=${inst.entityId} de05=${inst.de05id || 'none'} translate=(${txM}, ${tyM}, ${tzM})m`);
}

// Show final instance counts per definition
console.log(`\n=== FINAL INSTANCE COUNTS ===`);
const finalCounts = new Map();
for (const inst of instances) {
  const name = inst.defName || inst.resolvedName || '(unresolved)';
  finalCounts.set(name, (finalCounts.get(name) || 0) + 1);
}
const sorted = [...finalCounts.entries()].sort((a,b) => b[1]-a[1]);
for (const [name, count] of sorted.slice(0, 30)) {
  console.log(`  "${name}": ${count} instances`);
}
if (sorted.length > 30) console.log(`  ... and ${sorted.length - 30} more`);
