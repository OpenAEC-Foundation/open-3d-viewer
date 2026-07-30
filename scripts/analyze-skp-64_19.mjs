/**
 * Analyze the 64_19 data blocks to find how they reference definitions.
 * Compare matched instances (where we know the definition) with unmatched ones.
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

// Collect all instances with their tags
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

    // Read subsequent tags
    let scanPos = i + 110;
    let entityId = null, uuid = null, nextTag = null, nextData = null;

    // 67_19 entity ID
    if (scanPos+6 <= md.length && md[scanPos]===0x67 && md[scanPos+1]===0x19) {
      const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
      entityId = Array.from(md.slice(scanPos+6, scanPos+6+l)).map(b=>b.toString(16).padStart(2,'0')).join('');
      scanPos += 6 + l;
    }
    // 68_19 UUID
    if (scanPos+6 <= md.length && md[scanPos]===0x68 && md[scanPos+1]===0x19) {
      const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
      uuid = Array.from(md.slice(scanPos+6, scanPos+6+l)).map(b=>b.toString(16).padStart(2,'0')).join('');
      scanPos += 6 + l;
    }
    // Next tag after 67_19 + 68_19
    if (scanPos+6 <= md.length) {
      const tag = md[scanPos].toString(16).padStart(2,'0') + '_' + md[scanPos+1].toString(16).padStart(2,'0');
      const l = md[scanPos+2]|(md[scanPos+3]<<8)|(md[scanPos+4]<<16)|(md[scanPos+5]<<24);
      nextTag = tag;
      if (l >= 0 && l < 1000 && scanPos+6+l <= md.length) {
        nextData = md.slice(scanPos+6, scanPos+6+l);
      }
    }

    // Find 7e_15 name if present
    let defName = null;
    for (let j = scanPos; j < Math.min(scanPos + 300, md.length - 10); j++) {
      if (md[j]===0x7e && md[j+1]===0x15) {
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

    instances.push({ offset: i, entityId, uuid, nextTag, nextData, defName, vals });
  }
}

// Group by: definition-header instances (have 9e_13) vs reference instances
const defHeaderInst = instances.filter(i => i.nextTag === '9e_13');
const refInst = instances.filter(i => i.nextTag !== '9e_13');

console.log(`Total instances: ${instances.length}`);
console.log(`Definition-header instances (9e_13): ${defHeaderInst.length}`);
console.log(`Reference instances: ${refInst.length}`);
console.log(`  with 64_19: ${refInst.filter(i => i.nextTag === '64_19').length}`);
console.log(`  with 4c_1d: ${refInst.filter(i => i.nextTag === '4c_1d').length}`);
console.log(`  with 8d_13: ${refInst.filter(i => i.nextTag === '8d_13').length}`);
console.log(`  other: ${refInst.filter(i => !['64_19','4c_1d','8d_13'].includes(i.nextTag)).length}`);

// Build entity ID → name mapping from definition-header instances
const eidToName = new Map();
for (const inst of defHeaderInst) {
  if (inst.entityId && inst.defName) {
    eidToName.set(inst.entityId, inst.defName);
  }
}

// For reference instances with 64_19, dump the first few bytes and look for patterns
console.log(`\n=== 64_19 DATA ANALYSIS (matched instances) ===`);
const matched64 = refInst.filter(i => i.nextTag === '64_19' && eidToName.has(i.entityId));
for (const inst of matched64.slice(0, 10)) {
  const name = eidToName.get(inst.entityId);
  const hexStr = Array.from(inst.nextData.slice(0, 40)).map(b=>b.toString(16).padStart(2,'0')).join(' ');
  console.log(`  eid=${inst.entityId} → "${name}" data[0:40]=${hexStr}`);
}

console.log(`\n=== 64_19 DATA ANALYSIS (UNmatched instances) ===`);
const unmatched64 = refInst.filter(i => i.nextTag === '64_19' && !eidToName.has(i.entityId));
for (const inst of unmatched64.slice(0, 10)) {
  const hexStr = Array.from(inst.nextData.slice(0, 40)).map(b=>b.toString(16).padStart(2,'0')).join(' ');
  console.log(`  eid=${inst.entityId} data[0:40]=${hexStr}`);
}

console.log(`\n=== 4c_1d DATA ANALYSIS (first 10) ===`);
const inst4c = refInst.filter(i => i.nextTag === '4c_1d');
for (const inst of inst4c.slice(0, 10)) {
  const name = eidToName.get(inst.entityId) || '(unmatched)';
  const hexStr = Array.from(inst.nextData.slice(0, 40)).map(b=>b.toString(16).padStart(2,'0')).join(' ');
  console.log(`  eid=${inst.entityId} → "${name}" data[0:40]=${hexStr}`);
}

// Try to parse 64_19 as TLV sub-tags
console.log(`\n=== PARSING 64_19 AS SUB-TAGS (first matched instance) ===`);
if (matched64.length > 0) {
  const data = matched64[0].nextData;
  console.log(`Instance eid=${matched64[0].entityId} → "${eidToName.get(matched64[0].entityId)}" 64_19 len=${data.length}`);

  let p = 0;
  while (p < data.length - 6) {
    const tag = data[p].toString(16).padStart(2,'0') + '_' + data[p+1].toString(16).padStart(2,'0');
    const len = data[p+2]|(data[p+3]<<8)|(data[p+4]<<16)|(data[p+5]<<24);
    if (len < 0 || len > data.length - p - 6) {
      console.log(`  @${p}: INVALID tag=${tag} len=${len}`);
      break;
    }
    const preview = Array.from(data.slice(p+6, p+6+Math.min(len, 20))).map(b=>b.toString(16).padStart(2,'0')).join(' ');
    console.log(`  @${p}: ${tag} len=${len} data=[${preview}]${len > 20 ? '...' : ''}`);
    p += 6 + len;
  }
}

// Try to parse a 4c_1d block too
console.log(`\n=== PARSING 4c_1d AS SUB-TAGS (first instance) ===`);
if (inst4c.length > 0) {
  const data = inst4c[0].nextData;
  console.log(`Instance eid=${inst4c[0].entityId} → "${eidToName.get(inst4c[0].entityId) || '(unmatched)'}" 4c_1d len=${data.length}`);

  let p = 0;
  while (p < data.length - 6) {
    const tag = data[p].toString(16).padStart(2,'0') + '_' + data[p+1].toString(16).padStart(2,'0');
    const len = data[p+2]|(data[p+3]<<8)|(data[p+4]<<16)|(data[p+5]<<24);
    if (len < 0 || len > data.length - p - 6) {
      console.log(`  @${p}: INVALID tag=${tag} len=${len}`);
      break;
    }
    const preview = Array.from(data.slice(p+6, p+6+Math.min(len, 20))).map(b=>b.toString(16).padStart(2,'0')).join(' ');
    console.log(`  @${p}: ${tag} len=${len} data=[${preview}]${len > 20 ? '...' : ''}`);
    p += 6 + len;
  }
}

// Summary: For each tag type after 68_19, show if the instance has a 7e_15 name or not
console.log(`\n=== TAG TYPE vs NAME AVAILABILITY ===`);
const tagTypes = new Map();
for (const inst of instances) {
  const key = inst.nextTag || 'null';
  if (!tagTypes.has(key)) tagTypes.set(key, { named: 0, unnamed: 0 });
  const entry = tagTypes.get(key);
  if (inst.defName || eidToName.has(inst.entityId)) entry.named++;
  else entry.unnamed++;
}
for (const [tag, counts] of tagTypes) {
  console.log(`  ${tag}: ${counts.named} named, ${counts.unnamed} unnamed`);
}
