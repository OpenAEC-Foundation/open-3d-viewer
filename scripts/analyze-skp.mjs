/**
 * SKP Binary Format Analyzer
 *
 * Analyzes the definition/instance structure of a SketchUp file
 * to understand how components are organized.
 *
 * Usage: node scripts/analyze-skp.mjs <path-to-skp-file>
 */

import { readFileSync } from 'fs';
import { inflateSync } from 'zlib';

const filePath = process.argv[2];
if (!filePath) {
  console.error('Usage: node scripts/analyze-skp.mjs <path-to-skp-file>');
  process.exit(1);
}

console.log(`Analyzing: ${filePath}`);
const raw = readFileSync(filePath);
console.log(`File size: ${(raw.length / 1024 / 1024).toFixed(2)} MB`);

// Find ZIP inside SKP (scan for PK magic after header)
let zipStart = -1;
for (let i = 0; i < Math.min(raw.length, 500); i++) {
  if (raw[i] === 0x50 && raw[i + 1] === 0x4b && raw[i + 2] === 0x03 && raw[i + 3] === 0x04) {
    zipStart = i;
    break;
  }
}

if (zipStart < 0) {
  console.error('No ZIP archive found in SKP file');
  process.exit(1);
}

// Read header info
const headerStr = raw.slice(0, zipStart).toString('utf-8');
const versionMatch = headerStr.match(/\{(\d+\.\d+\.\d+)\}/);
console.log(`SKP version: ${versionMatch ? versionMatch[1] : 'unknown'}`);
console.log(`ZIP starts at offset: ${zipStart}`);

// Extract model data from ZIP
// Simple ZIP parser - find local file headers
const entries = [];
let pos = zipStart;
while (pos < raw.length - 30) {
  if (raw[pos] !== 0x50 || raw[pos + 1] !== 0x4b || raw[pos + 2] !== 0x03 || raw[pos + 3] !== 0x04) break;

  const comprMethod = raw[pos + 8] | (raw[pos + 9] << 8);
  const compSize = raw[pos + 18] | (raw[pos + 19] << 8) | (raw[pos + 20] << 16) | (raw[pos + 21] << 24);
  const uncompSize = raw[pos + 22] | (raw[pos + 23] << 8) | (raw[pos + 24] << 16) | (raw[pos + 25] << 24);
  const nameLen = raw[pos + 26] | (raw[pos + 27] << 8);
  const extraLen = raw[pos + 28] | (raw[pos + 29] << 8);
  const name = raw.slice(pos + 30, pos + 30 + nameLen).toString('utf-8');
  const dataStart = pos + 30 + nameLen + extraLen;

  entries.push({ name, comprMethod, compSize, uncompSize, dataStart });
  pos = dataStart + compSize;
}

console.log(`\nZIP entries: ${entries.length}`);
for (const e of entries) {
  console.log(`  ${e.name} (${(e.uncompSize / 1024).toFixed(0)} KB, method=${e.comprMethod})`);
}

// Find and decompress model data (largest entry, usually "model" or "mdl" or section0)
const modelEntry = entries.reduce((best, e) => (!best || e.uncompSize > best.uncompSize) ? e : best, null);
if (!modelEntry) {
  console.error('No model data found');
  process.exit(1);
}

console.log(`\nUsing entry: "${modelEntry.name}" (${(modelEntry.uncompSize / 1024 / 1024).toFixed(2)} MB uncompressed)`);

let md;
if (modelEntry.comprMethod === 8) {
  // Raw deflate — try with and without header
  try {
    md = inflateSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize));
  } catch {
    // Try raw inflate (wbits = -MAX_WBITS equivalent via createInflateRaw)
    const { inflateRawSync } = await import('zlib');
    md = inflateRawSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize));
  }
} else {
  md = raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize);
}

console.log(`Model data: ${(md.length / 1024 / 1024).toFixed(2)} MB`);

// ======================================================================
// 1. Find all 7e_15 tags (definition names / instance references)
// ======================================================================
const allNames = [];
for (let i = 0; i < md.length - 10; i++) {
  if (md[i] === 0x7e && md[i + 1] === 0x15) {
    const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
    if (len > 0 && len < 300 && i + 6 + len <= md.length) {
      const name = Buffer.from(md.slice(i + 6, i + 6 + len)).toString('utf-8').replace(/\0/g, '').trim();
      if (name) {
        allNames.push({ name, offset: i, endOffset: i + 6 + len });
      }
    }
  }
}
console.log(`\n=== 7e_15 name tags: ${allNames.length} total ===`);

// Count by name
const nameCounts = new Map();
for (const n of allNames) {
  nameCounts.set(n.name, (nameCounts.get(n.name) || 0) + 1);
}
const sortedNames = [...nameCounts.entries()].sort((a, b) => b[1] - a[1]);
for (const [name, count] of sortedNames.slice(0, 30)) {
  console.log(`  "${name}": ${count} occurrences`);
}

// ======================================================================
// 2. Find all 7c_15 geometry blocks
// ======================================================================
const geomBlocks = [];
for (let i = 0; i < md.length - 6; i++) {
  if (md[i] === 0x7c && md[i + 1] === 0x15) {
    const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
    if (len > 100 && len < md.length && i + 6 + len <= md.length) {
      geomBlocks.push({ offset: i, dataStart: i + 6, dataEnd: i + 6 + len, len });
    }
  }
}
console.log(`\n=== 7c_15 geometry blocks: ${geomBlocks.length} ===`);

// Associate each geometry block with preceding 7e_15
const defBlocks = [];
for (const gb of geomBlocks) {
  let bestDef = null;
  for (const n of allNames) {
    if (n.offset < gb.offset) bestDef = n;
    else break;
  }
  defBlocks.push({
    defName: bestDef?.name ?? '__unassigned__',
    defOffset: bestDef?.offset ?? 0,
    geomOffset: gb.offset,
    geomLen: gb.len,
  });
}

// Group geometry by definition
const geomByDef = new Map();
for (const db of defBlocks) {
  if (!geomByDef.has(db.defName)) geomByDef.set(db.defName, []);
  geomByDef.get(db.defName).push(db);
}
console.log(`Geometry blocks by definition (${geomByDef.size} defs):`);
const sortedGeom = [...geomByDef.entries()].sort((a, b) => {
  const aSize = a[1].reduce((s, g) => s + g.geomLen, 0);
  const bSize = b[1].reduce((s, g) => s + g.geomLen, 0);
  return bSize - aSize;
});
for (const [name, blocks] of sortedGeom.slice(0, 20)) {
  const totalSize = blocks.reduce((s, g) => s + g.geomLen, 0);
  console.log(`  "${name}": ${blocks.length} blocks, ${(totalSize / 1024).toFixed(0)} KB, name@${blocks[0].defOffset}, geom@${blocks[0].geomOffset}`);
}

// ======================================================================
// 3. Find all 66_19 instance transforms
// ======================================================================
const dv = new DataView(md.buffer, md.byteOffset, md.byteLength);
const instances = [];
for (let i = 0; i < md.length - 110; i++) {
  if (md[i] === 0x66 && md[i + 1] === 0x19) {
    const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
    if (len !== 104) continue;

    const vals = [];
    let valid = true;
    for (let k = 0; k < 13; k++) {
      const v = dv.getFloat64(i + 6 + k * 8, true);
      if (!isFinite(v)) { valid = false; break; }
      vals.push(v);
    }
    if (!valid) continue;
    if (Math.abs(vals[12] - 1.0) > 0.01) continue;

    // Find definition name forward
    let defName = '';
    for (let j = i + 110; j < Math.min(i + 20000, md.length - 10); j++) {
      if (md[j] === 0x7e && md[j + 1] === 0x15) {
        const nlen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
        if (nlen > 0 && nlen < 300 && j + 6 + nlen <= md.length) {
          defName = Buffer.from(md.slice(j + 6, j + 6 + nlen)).toString('utf-8').replace(/\0/g, '').trim();
          break;
        }
      }
      if (md[j] === 0x66 && md[j + 1] === 0x19) {
        const nextLen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
        if (nextLen === 104) break;
      }
    }

    // Fallback: backward scan
    if (!defName) {
      for (let j = i - 1; j >= Math.max(0, i - 5000); j--) {
        if (md[j] === 0x7e && md[j + 1] === 0x15) {
          const nlen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (nlen > 0 && nlen < 300 && j + 6 + nlen <= md.length) {
            defName = Buffer.from(md.slice(j + 6, j + 6 + nlen)).toString('utf-8').replace(/\0/g, '').trim();
            break;
          }
        }
        if (md[j] === 0x66 && md[j + 1] === 0x19) break;
      }
    }

    const tx = vals[9], ty = vals[10], tz = vals[11];
    const isIdentity = Math.abs(vals[0] - 1) < 1e-6 && Math.abs(vals[4] - 1) < 1e-6 &&
                        Math.abs(vals[8] - 1) < 1e-6 && Math.abs(tx) < 0.01 && Math.abs(ty) < 0.01 && Math.abs(tz) < 0.01;

    instances.push({
      defName: defName || '(unnamed)',
      offset: i,
      tx, ty, tz,
      isIdentity,
      transform: vals,
    });
  }
}

console.log(`\n=== 66_19 instance transforms: ${instances.length} ===`);

// Group by definition name
const instByDef = new Map();
for (const inst of instances) {
  if (!instByDef.has(inst.defName)) instByDef.set(inst.defName, []);
  instByDef.get(inst.defName).push(inst);
}

const sortedInst = [...instByDef.entries()].sort((a, b) => b[1].length - a[1].length);
for (const [name, insts] of sortedInst.slice(0, 30)) {
  const hasGeom = geomByDef.has(name);
  console.log(`  "${name}": ${insts.length} instances ${hasGeom ? '(HAS geometry def)' : '(NO geometry def!)'}`);
  for (const inst of insts.slice(0, 5)) {
    const txM = (inst.tx * 0.0254).toFixed(3);
    const tyM = (inst.ty * 0.0254).toFixed(3);
    const tzM = (inst.tz * 0.0254).toFixed(3);
    console.log(`    @offset ${inst.offset}: translate=(${txM}, ${tyM}, ${tzM})m ${inst.isIdentity ? '[IDENTITY]' : ''}`);
  }
  if (insts.length > 5) console.log(`    ... and ${insts.length - 5} more`);
}

// ======================================================================
// 4. Nesting analysis — which instances are INSIDE which definitions?
// ======================================================================
console.log(`\n=== NESTING ANALYSIS ===`);

// Build definition sections: from 7e_15 name tag to start of next definition
const defNameOffsets = new Map();
for (const db of defBlocks) {
  if (db.defName === '__unassigned__') continue;
  const existing = defNameOffsets.get(db.defName);
  if (!existing || db.defOffset < existing) {
    defNameOffsets.set(db.defName, db.defOffset);
  }
}

const sortedDefs = [...defNameOffsets.entries()].sort((a, b) => a[1] - b[1]);
const maxDataEnd = defBlocks.reduce((mx, b) => Math.max(mx, b.geomOffset + b.geomLen), 0);

const defSections = new Map();
for (let i = 0; i < sortedDefs.length; i++) {
  const [name, start] = sortedDefs[i];
  const end = i + 1 < sortedDefs.length ? sortedDefs[i + 1][1] : maxDataEnd;
  defSections.set(name, { start, end });
}

console.log(`Definition sections (${defSections.size}):`);
const sortedSections = [...defSections.entries()].sort((a, b) => (b[1].end - b[1].start) - (a[1].end - a[1].start));
for (const [name, range] of sortedSections.slice(0, 15)) {
  console.log(`  "${name}": offset ${range.start}–${range.end} (${((range.end - range.start) / 1024).toFixed(0)} KB)`);
}

// Classify each instance
let nestedCount = 0;
let rootCount = 0;
for (const inst of instances) {
  let bestParent = null;
  let bestSize = Infinity;
  for (const [secName, range] of defSections) {
    if (secName === inst.defName) continue;
    if (inst.offset >= range.start && inst.offset < range.end) {
      const size = range.end - range.start;
      if (size < bestSize) {
        bestSize = size;
        bestParent = secName;
      }
    }
  }
  inst.parentDef = bestParent;
  if (bestParent) nestedCount++;
  else rootCount++;
}

console.log(`\nRoot instances: ${rootCount}, Nested instances: ${nestedCount}`);

// Show nesting relationships
const byParent = new Map();
for (const inst of instances) {
  if (inst.parentDef) {
    const key = `${inst.parentDef} → ${inst.defName}`;
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(inst);
  }
}

for (const [rel, insts] of [...byParent.entries()].sort((a, b) => b[1].length - a[1].length).slice(0, 20)) {
  console.log(`  ${rel}: ${insts.length} instances`);
  for (const inst of insts.slice(0, 3)) {
    const txM = (inst.tx * 0.0254).toFixed(3);
    const tyM = (inst.ty * 0.0254).toFixed(3);
    const tzM = (inst.tz * 0.0254).toFixed(3);
    console.log(`    translate=(${txM}, ${tyM}, ${tzM})m @offset ${inst.offset}`);
  }
}

// ======================================================================
// 5. Show definitions that have geometry but NO instances (identity defs)
// ======================================================================
console.log(`\n=== IDENTITY DEFINITIONS (have geometry, no instances) ===`);
for (const [defName, blocks] of sortedGeom) {
  if (!instByDef.has(defName)) {
    const totalSize = blocks.reduce((s, g) => s + g.geomLen, 0);
    console.log(`  "${defName}": ${blocks.length} blocks, ${(totalSize / 1024).toFixed(0)} KB — RENDERED AT ORIGIN`);
  }
}

// ======================================================================
// 6. Show definitions that have instances but NO geometry (missing defs)
// ======================================================================
console.log(`\n=== MISSING DEFINITIONS (instances but no geometry) ===`);
for (const [defName, insts] of sortedInst) {
  if (!geomByDef.has(defName)) {
    console.log(`  "${defName}": ${insts.length} instances but NO geometry definition found`);
  }
}

// ======================================================================
// 7. Summary: what SHOULD be rendered
// ======================================================================
console.log(`\n=== RENDERING SUMMARY ===`);
console.log(`Definitions with geometry: ${geomByDef.size}`);
console.log(`Definitions with instances: ${instByDef.size}`);
console.log(`Definitions with BOTH geometry + instances: ${[...geomByDef.keys()].filter(k => instByDef.has(k)).length}`);
console.log(`Identity definitions (geometry, no instances): ${[...geomByDef.keys()].filter(k => !instByDef.has(k)).length}`);
console.log(`\nFor correct rendering:`);
console.log(`  - Root instances: render definition geometry with instance transform`);
console.log(`  - Nested instances: compose parent_transform × child_transform`);
console.log(`  - Identity definitions (large, no instances): render at identity (main building)`);
console.log(`  - Identity definitions (small, no instances): SKIP (likely missing instance refs)`);
