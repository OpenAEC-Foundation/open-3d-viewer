/**
 * Match unresolved entity IDs to definitions by scanning the full file.
 * For each 7e_15 definition name, look at ALL 67_19 tags within 500 bytes before it
 * and also scan within the 7c_15 geometry block headers for entity ID references.
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

// Find all 67_19 entity ID tags in the entire file
const allEntityIds = [];
for (let i = 0; i < md.length - 6; i++) {
  if (md[i] === 0x67 && md[i+1] === 0x19) {
    const len = md[i+2]|(md[i+3]<<8)|(md[i+4]<<16)|(md[i+5]<<24);
    if (len > 0 && len <= 8 && i+6+len <= md.length) {
      const eid = Array.from(md.slice(i+6, i+6+len)).map(b => b.toString(16).padStart(2,'0')).join('');
      allEntityIds.push({ offset: i, eid, len: len });
    }
  }
}
console.log(`Total 67_19 tags in file: ${allEntityIds.length}`);

// Find all 7e_15 definition names
const allDefs = [];
for (let i = 0; i < md.length - 10; i++) {
  if (md[i]===0x7e && md[i+1]===0x15) {
    const len = md[i+2]|(md[i+3]<<8)|(md[i+4]<<16)|(md[i+5]<<24);
    if (len > 0 && len < 300 && i+6+len <= md.length) {
      const name = Buffer.from(md.slice(i+6, i+6+len)).toString('utf-8').replace(/\0/g,'').trim();
      if (name) allDefs.push({ name, offset: i });
    }
  }
}
console.log(`Total 7e_15 definitions: ${allDefs.length}`);

// Find all 66_19 instances
const allInstances = [];
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
    allInstances.push({ offset: i, vals });
  }
}

// For each 67_19, check if it immediately follows a 66_19 (= instance entity ID)
// or immediately precedes 7e_15 cluster (= definition entity ID)
const instanceEids = new Set(); // entity ID tags that follow 66_19
for (const inst of allInstances) {
  const afterEnd = inst.offset + 110;
  for (const eidTag of allEntityIds) {
    if (eidTag.offset === afterEnd) {
      inst.entityId = eidTag.eid;
      instanceEids.add(eidTag.offset);
      break;
    }
  }
}

// Non-instance 67_19 tags (definition entity IDs or other references)
const nonInstanceEids = allEntityIds.filter(e => !instanceEids.has(e.offset));
console.log(`\n67_19 tags after 66_19 (instance refs): ${instanceEids.size}`);
console.log(`67_19 tags NOT after 66_19 (other): ${nonInstanceEids.length}`);

// Map: for each non-instance 67_19, find the nearest FOLLOWING 7e_15 definition
const eidToDefName = new Map();
for (const eidTag of nonInstanceEids) {
  // Find nearest 7e_15 within 200 bytes forward
  for (const def of allDefs) {
    if (def.offset > eidTag.offset && def.offset < eidTag.offset + 200) {
      eidToDefName.set(eidTag.eid, { name: def.name, eidOffset: eidTag.offset, defOffset: def.offset });
      break;
    }
  }
}

console.log(`\nNon-instance 67_19 → nearest 7e_15 mapping (${eidToDefName.size} entries):`);
const sortedMapping = [...eidToDefName.entries()].sort((a,b) => a[1].eidOffset - b[1].eidOffset);
for (const [eid, info] of sortedMapping.slice(0, 30)) {
  console.log(`  ${eid} → "${info.name}" (67_19@${info.eidOffset}, 7e_15@${info.defOffset}, gap=${info.defOffset - info.eidOffset})`);
}
if (sortedMapping.length > 30) console.log(`  ... and ${sortedMapping.length - 30} more`);

// ========================================================
// Now match instance entity IDs to definition entity IDs
// ========================================================
// Some instances have entity IDs that DON'T directly match the definition-header entity IDs.
// This happens with Groups, where the instance references the Group entity,
// not the ComponentDefinition entity.
//
// Strategy: For each non-instance 67_19 tag, also look BACKWARD to find the nearest
// 66_19 instance with the SAME parent context. This links the definition entity ID
// to the instance entity ID that appears in the same context (same 66_19 preceding it).

console.log(`\n=== FINAL MATCHING: Instance EID → Definition ===`);

// Build: definition entity ID → definition name (from non-instance 67_19 tags near 7e_15)
const defEidToName = new Map();
for (const [eid, info] of eidToDefName) {
  defEidToName.set(eid, info.name);
}

// For named instances: instance entity ID = definition entity ID (same)
// For unnamed instances: instance entity ID ≠ definition entity ID
// We need to find the link.
//
// The structure for a "first definition declaration" is:
// 66_19 (transform, instance entity ID = X)
//   → 67_19 (X)
//   → 68_19 (UUID)
//   → 64_19 (data) — might contain the definition reference
// Then immediately:
// 66_19 (transform, instance entity ID = X again)
//   → 67_19 (X)
//   → 68_19 (UUID)
//   → 9e_13...7e_15 (definition header with definition entity ID = Y)
//
// So the instance entity ID X appears twice: once in the first 66_19, once in the
// definition-header 66_19. The definition-header also has entity ID Y (in 67_19
// inside the header chain).

// Let me match by looking at 66_19 instances that immediately precede definition headers.
// If a 66_19 at offset A has entity ID X, and the definition header that follows has
// entity ID Y (non-instance 67_19), then instance entity ID X maps to definition entity ID Y.

// Find instances whose 67_19 matches a definition's 67_19
console.log(`\nDirect instance-eid → definition-eid matches:`);
let directMatches = 0;
const instEidToDefName = new Map();

for (const inst of allInstances) {
  if (!inst.entityId) continue;
  if (defEidToName.has(inst.entityId)) {
    // Direct match: instance entity ID is the definition entity ID
    instEidToDefName.set(inst.entityId, defEidToName.get(inst.entityId));
    directMatches++;
  }
}
console.log(`Direct matches: ${directMatches} instances`);
console.log(`Unique direct mappings: ${instEidToDefName.size}`);

// For remaining: look for 66_19 instances that are immediately followed by a definition header
// (this is the "second instance in a pair" pattern)
console.log(`\nLooking for instance → definition pairs...`);
for (const inst of allInstances) {
  if (!inst.entityId) continue;
  if (instEidToDefName.has(inst.entityId)) continue; // already matched

  // Check if this instance is followed by definition header tags (9e_13...7e_15)
  const afterEnd = inst.offset + 110;
  // Skip the 67_19 and 68_19 tags
  let scanPos = afterEnd;
  // 67_19
  if (scanPos+6 <= md.length && md[scanPos]===0x67 && md[scanPos+1]===0x19) {
    const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
    scanPos += 6 + l;
  }
  // 68_19
  if (scanPos+6 <= md.length && md[scanPos]===0x68 && md[scanPos+1]===0x19) {
    const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
    scanPos += 6 + l;
  }

  // Check if next tag is 9e_13 (definition header marker)
  if (scanPos+2 <= md.length && md[scanPos]===0x9e && md[scanPos+1]===0x13) {
    // This instance IS a definition header! Find the 7e_15 name
    for (let j = scanPos; j < Math.min(scanPos + 300, md.length - 10); j++) {
      if (md[j]===0x7e && md[j+1]===0x15) {
        const nlen = md[j+2]|(md[j+3]<<8)|(md[j+4]<<16)|(md[j+5]<<24);
        if (nlen > 0 && nlen < 300 && j+6+nlen <= md.length) {
          const name = Buffer.from(md.slice(j+6, j+6+nlen)).toString('utf-8').replace(/\0/g,'').trim();
          instEidToDefName.set(inst.entityId, name);
          break;
        }
      }
    }
  }
}

console.log(`Total instance-eid → definition mappings: ${instEidToDefName.size}`);
for (const [eid, name] of [...instEidToDefName.entries()].slice(0, 30)) {
  console.log(`  ${eid} → "${name}"`);
}

// Now: find instances whose entity ID is NOT in instEidToDefName
// These must use a DIFFERENT entity ID than the definition header
// Look for the "pair" pattern: two consecutive 66_19 with same entity ID,
// second one has definition header
const unmatchedEids = new Set();
for (const inst of allInstances) {
  if (inst.entityId && !instEidToDefName.has(inst.entityId)) {
    unmatchedEids.add(inst.entityId);
  }
}
console.log(`\nUnmatched instance entity IDs: ${unmatchedEids.size}`);

// For each unmatched entity ID: find ALL occurrences of it as 67_19 in the file
// and check if any occurrence is near a definition header
for (const eid of [...unmatchedEids].slice(0, 20)) {
  const occurrences = allEntityIds.filter(e => e.eid === eid);
  console.log(`\n  Entity ID ${eid} appears ${occurrences.length} times:`);
  for (const occ of occurrences) {
    // What's within 50 bytes of this occurrence?
    const nearbyTags = [];
    for (let j = Math.max(0, occ.offset - 20); j < Math.min(md.length - 6, occ.offset + occ.len + 6 + 50); j++) {
      for (const def of allDefs) {
        if (def.offset === j) nearbyTags.push(`7e_15("${def.name}")@${j}`);
      }
    }
    // Check if preceded by 66_19
    let preceded66 = false;
    for (const inst of allInstances) {
      if (inst.offset + 110 === occ.offset) { preceded66 = true; break; }
    }
    console.log(`    @${occ.offset} ${preceded66 ? '[after 66_19]' : '[standalone]'} ${nearbyTags.length > 0 ? nearbyTags.join(', ') : ''}`);
  }
}
