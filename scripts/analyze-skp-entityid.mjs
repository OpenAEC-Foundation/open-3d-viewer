/**
 * Entity ID mapping analysis — maps 67_19 entity IDs to 7e_15 definition names.
 * This reveals how unnamed instances reference their definitions.
 */

import { readFileSync } from 'fs';
import { inflateRawSync, inflateSync } from 'zlib';

const filePath = process.argv[2];
if (!filePath) { console.error('Usage: node analyze-skp-entityid.mjs <file>'); process.exit(1); }

const raw = readFileSync(filePath);
let zipStart = -1;
for (let i = 0; i < 500; i++) {
  if (raw[i]===0x50 && raw[i+1]===0x4b && raw[i+2]===0x03 && raw[i+3]===0x04) { zipStart = i; break; }
}

const entries = [];
let pos = zipStart;
while (pos < raw.length - 30) {
  if (raw[pos]!==0x50||raw[pos+1]!==0x4b||raw[pos+2]!==0x03||raw[pos+3]!==0x04) break;
  const comprMethod = raw[pos+8]|(raw[pos+9]<<8);
  const compSize = raw[pos+18]|(raw[pos+19]<<8)|(raw[pos+20]<<16)|(raw[pos+21]<<24);
  const nameLen = raw[pos+26]|(raw[pos+27]<<8);
  const extraLen = raw[pos+28]|(raw[pos+29]<<8);
  const name = raw.slice(pos+30, pos+30+nameLen).toString('utf-8');
  const dataStart = pos+30+nameLen+extraLen;
  entries.push({ name, comprMethod, compSize, dataStart });
  pos = dataStart + compSize;
}

const modelEntry = entries.find(e => e.name === 'model.dat') || entries.reduce((b,e) => (!b||e.compSize>b.compSize)?e:b, null);
let md;
try { md = inflateSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize)); }
catch { md = inflateRawSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize)); }

const dv = new DataView(md.buffer, md.byteOffset, md.byteLength);

// ========================================================
// Step 1: Find all 66_19 instances with their 67_19 entity IDs
// ========================================================
const instances = [];
for (let i = 0; i < md.length - 110; i++) {
  if (md[i] === 0x66 && md[i+1] === 0x19) {
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

    // Read 67_19 entity ID immediately after (at offset i + 110)
    let entityId = null;
    const afterEnd = i + 110;
    if (afterEnd + 6 <= md.length && md[afterEnd] === 0x67 && md[afterEnd+1] === 0x19) {
      const eidLen = md[afterEnd+2]|(md[afterEnd+3]<<8)|(md[afterEnd+4]<<16)|(md[afterEnd+5]<<24);
      if (eidLen > 0 && eidLen <= 8) {
        entityId = Array.from(md.slice(afterEnd+6, afterEnd+6+eidLen))
          .map(b => b.toString(16).padStart(2,'0')).join('');
      }
    }

    // Also scan forward for 7e_15 name (as before)
    let defName = '';
    for (let j = afterEnd; j < Math.min(afterEnd + 500, md.length - 10); j++) {
      if (md[j] === 0x7e && md[j+1] === 0x15) {
        const nlen = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (nlen > 0 && nlen < 300 && j+6+nlen <= md.length) {
          defName = Buffer.from(md.slice(j+6, j+6+nlen)).toString('utf-8').replace(/\0/g,'').trim();
          break;
        }
      }
      if (md[j]===0x66 && md[j+1]===0x19) {
        const nl = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (nl === 104) break;
      }
    }

    instances.push({
      offset: i,
      entityId,
      defName: defName || null,
      tx: vals[9], ty: vals[10], tz: vals[11],
      transform: vals,
    });
  }
}

console.log(`Total instances: ${instances.length}`);
console.log(`With 67_19 entity ID: ${instances.filter(i => i.entityId).length}`);
console.log(`With 7e_15 name: ${instances.filter(i => i.defName).length}`);
console.log(`Unnamed (no 7e_15): ${instances.filter(i => !i.defName).length}\n`);

// ========================================================
// Step 2: Build entity ID → definition name mapping from named instances
// ========================================================
const entityToName = new Map();
for (const inst of instances) {
  if (inst.entityId && inst.defName) {
    if (!entityToName.has(inst.entityId)) {
      entityToName.set(inst.entityId, inst.defName);
    }
  }
}

console.log(`=== Entity ID → Definition Name mapping (${entityToName.size} entries) ===`);
for (const [eid, name] of entityToName) {
  console.log(`  ${eid} → "${name}"`);
}

// ========================================================
// Step 3: Resolve unnamed instances using entity ID mapping
// ========================================================
console.log(`\n=== RESOLVING UNNAMED INSTANCES ===`);
let resolved = 0, unresolved = 0;
const unresolvedIds = new Map();

for (const inst of instances) {
  if (!inst.defName && inst.entityId) {
    const mapped = entityToName.get(inst.entityId);
    if (mapped) {
      inst.defName = mapped;
      inst.resolvedViaEntityId = true;
      resolved++;
    } else {
      unresolved++;
      unresolvedIds.set(inst.entityId, (unresolvedIds.get(inst.entityId)||0) + 1);
    }
  }
}

console.log(`Resolved via entity ID: ${resolved}`);
console.log(`Still unresolved: ${unresolved}`);

if (unresolvedIds.size > 0) {
  console.log(`\nUnresolved entity IDs:`);
  for (const [eid, count] of [...unresolvedIds.entries()].sort((a,b) => b[1]-a[1])) {
    console.log(`  ${eid}: ${count} instances`);
  }
}

// ========================================================
// Step 4: Find entity IDs for definitions that have no instances
// Look for 67_19 tags in the definition headers themselves
// ========================================================
console.log(`\n=== SCANNING FOR DEFINITION ENTITY IDs ===`);

// Find all 7e_15 definition names
const allDefs = [];
for (let i = 0; i < md.length - 10; i++) {
  if (md[i]===0x7e && md[i+1]===0x15) {
    const len = md[i+2]|(md[i+3]<<8)|(md[i+4]<<16)|(md[i+5]<<24);
    if (len > 0 && len < 300 && i+6+len <= md.length) {
      const name = Buffer.from(md.slice(i+6, i+6+len)).toString('utf-8').replace(/\0/g,'').trim();
      if (name) allDefs.push({ name, offset: i, endOffset: i+6+len });
    }
  }
}

// For each definition, look BACKWARD to find the nearest 67_19 before it
// The structure before a definition name should include entity ID info
const defEntityIds = new Map();
for (const def of allDefs) {
  // Scan backward from 7e_15 to find tags
  // The sequence before 7e_15 is usually: ...a0_13, 7d_15, 7e_15
  // And before that: 9f_13, 9b_13, 8e_13, 9e_13
  // And before THAT: 68_19, 67_19 (the entity IDs)

  // Look for 67_19 in the 200 bytes before 7e_15
  for (let j = def.offset - 1; j >= Math.max(0, def.offset - 200); j--) {
    if (md[j] === 0x67 && md[j+1] === 0x19) {
      const eidLen = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
      if (eidLen > 0 && eidLen <= 8) {
        const eid = Array.from(md.slice(j+6, j+6+eidLen))
          .map(b => b.toString(16).padStart(2,'0')).join('');
        defEntityIds.set(def.name, eid);
        break;
      }
    }
  }
}

console.log(`Found entity IDs for ${defEntityIds.size}/${allDefs.length} definitions`);

// Check which unresolved entity IDs match definitions
const defIdToName = new Map();
for (const [name, eid] of defEntityIds) {
  defIdToName.set(eid, name);
}

// Now resolve the remaining unnamed instances
let newlyResolved = 0;
for (const inst of instances) {
  if (!inst.defName && inst.entityId) {
    const mapped = defIdToName.get(inst.entityId);
    if (mapped) {
      inst.defName = mapped;
      inst.resolvedViaDefScan = true;
      newlyResolved++;
    }
  }
}
console.log(`Newly resolved via definition scan: ${newlyResolved}`);
console.log(`Total resolved: ${resolved + newlyResolved}/${resolved + newlyResolved + unresolved - newlyResolved}`);

// ========================================================
// Step 5: Final summary
// ========================================================
console.log(`\n=== FINAL INSTANCE SUMMARY ===`);
const finalByDef = new Map();
for (const inst of instances) {
  const name = inst.defName || '(still unnamed)';
  if (!finalByDef.has(name)) finalByDef.set(name, []);
  finalByDef.get(name).push(inst);
}

const sortedFinal = [...finalByDef.entries()].sort((a,b) => b[1].length - a[1].length);
for (const [name, insts] of sortedFinal.slice(0, 30)) {
  console.log(`  "${name}": ${insts.length} instances`);
  for (const inst of insts.slice(0, 3)) {
    const txM = (inst.tx * 0.0254).toFixed(3);
    const tyM = (inst.ty * 0.0254).toFixed(3);
    const tzM = (inst.tz * 0.0254).toFixed(3);
    const via = inst.resolvedViaEntityId ? ' [via entity ID from named inst]' :
                inst.resolvedViaDefScan ? ' [via definition scan]' : '';
    console.log(`    translate=(${txM}, ${tyM}, ${tzM})m @${inst.offset} eid=${inst.entityId}${via}`);
  }
  if (insts.length > 3) console.log(`    ... and ${insts.length - 3} more`);
}

// Show remaining identity definitions vs. their entity IDs
console.log(`\n=== UNRESOLVED ENTITY ID ↔ DEFINITION CROSS-REFERENCE ===`);
const stillUnnamed = instances.filter(i => !i.defName);
if (stillUnnamed.length > 0) {
  const unresEids = new Set(stillUnnamed.map(i => i.entityId).filter(Boolean));
  console.log(`${stillUnnamed.length} unnamed instances with ${unresEids.size} unique entity IDs`);

  // Show which entity IDs are unresolved and check if they match any definition
  for (const eid of unresEids) {
    const defMatch = defIdToName.get(eid);
    const count = stillUnnamed.filter(i => i.entityId === eid).length;
    console.log(`  ${eid}: ${count} instances → ${defMatch ? `"${defMatch}"` : '(no match)'}`);
  }
}
