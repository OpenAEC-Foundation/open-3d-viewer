/**
 * SKP Loader Test Protocol
 * Tests the SKP loader logic against multiple SKP files to verify:
 * 1. ZIP extraction works
 * 2. Vertices, edges, faces are extracted
 * 3. Definition blocks are found and named
 * 4. Instance transforms are found and linked
 * 5. Face classification (root vs definition) works
 * 6. Spatial outlier filtering works
 * 7. Final rendered face counts are reasonable
 */
import { readFileSync, readdirSync } from 'fs';
import { unzipSync } from 'fflate';
import path from 'path';

const INCHES_TO_METERS = 0.0254;

// ============================================================
// Replicated loader logic (matching SKPLoader.ts exactly)
// ============================================================

function validateSKPHeader(buf) {
  if (buf.length < 65) return false;
  if (buf[0] !== 0xFF || buf[1] !== 0xFE || buf[2] !== 0xFF) return false;
  const marker = String.fromCharCode(buf[4], buf[6], buf[8], buf[10], buf[12], buf[14], buf[16], buf[18]);
  return marker === 'SketchUp';
}

function parseSKPVersion(buf) {
  try {
    for (let i = 20; i < Math.min(60, buf.length - 10); i++) {
      if (buf[i] === 0xFF && buf[i + 1] === 0xFE && buf[i + 2] === 0xFF) {
        const strLen = buf[i + 3];
        if (strLen > 0 && strLen < 20 && i + 4 + strLen * 2 <= buf.length) {
          let versionStr = '';
          for (let j = 0; j < strLen; j++) {
            versionStr += String.fromCharCode(buf[i + 4 + j * 2]);
          }
          const match = versionStr.match(/\{(\d+)\./);
          if (match) return { major: parseInt(match[1]), versionStr };
        }
      }
    }
  } catch {}
  return { major: 0, versionStr: 'unknown' };
}

function findZipOffset(buf) {
  for (let i = 40; i < Math.min(buf.length, 200); i++) {
    if (buf[i] === 0x50 && buf[i + 1] === 0x4B && buf[i + 2] === 0x03 && buf[i + 3] === 0x04) {
      return i;
    }
  }
  return -1;
}

function detectEntityIdSize(md) {
  let count2 = 0, count3 = 0;
  const limit = Math.min(md.length - 30, 500000);
  for (let i = 0; i < limit; i++) {
    if (md[i] === 0xc5 && md[i + 1] === 0x09 &&
        md[i + 2] === 0x18 && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      for (let j = i - 20; j < i; j++) {
        if (j >= 0 && md[j] === 0xde && md[j + 1] === 0x05) {
          const len = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (len === 2) count2++;
          else if (len === 3) count3++;
          break;
        }
      }
      if (count2 + count3 >= 20) break;
    }
  }
  return count3 > count2 ? 3 : count2 > 0 ? 2 : 3;
}

function readEntityId(md, offset, size) {
  if (size === 3) return (md[offset] << 16) | (md[offset + 1] << 8) | md[offset + 2];
  return (md[offset] << 8) | md[offset + 1];
}

function extractVertices(md, dv, idSize) {
  const positions = new Map();
  for (let i = 0; i < md.length - 30; i++) {
    if (md[i] === 0xc5 && md[i + 1] === 0x09 &&
        md[i + 2] === 0x18 && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      for (let j = i - 20; j < i; j++) {
        if (j >= 0 && md[j] === 0xde && md[j + 1] === 0x05 && md[j + 3] === 0x00 &&
            md[j + 4] === 0x00 && md[j + 5] === 0x00) {
          const delen = md[j + 2];
          if (delen >= 1 && delen <= 3) {
            const id = readEntityId(md, j + 6, delen);
            positions.set(id, {
              x: dv.getFloat64(i + 6, true),
              y: dv.getFloat64(i + 14, true),
              z: dv.getFloat64(i + 22, true),
            });
          }
          break;
        }
      }
    }
  }
  return positions;
}

function extractEdges(md, idSize) {
  const edges = new Map();
  for (let i = 0; i < md.length - 20; i++) {
    if (md[i] === 0xb9 && md[i + 1] === 0x0b && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      const b9len = md[i + 2];
      if (b9len < 1 || b9len > 3) continue;
      const v1 = readEntityId(md, i + 6, b9len);
      const searchStart = i + 6 + b9len;
      for (let j = searchStart; j < Math.min(searchStart + 12, md.length - 9); j++) {
        if (md[j] === 0xba && md[j + 1] === 0x0b && md[j + 3] === 0x00 &&
            md[j + 4] === 0x00 && md[j + 5] === 0x00) {
          const balen = md[j + 2];
          if (balen < 1 || balen > 3) break;
          const v2 = readEntityId(md, j + 6, balen);
          for (let k = i - 30; k < i; k++) {
            if (k >= 0 && md[k] === 0xde && md[k + 1] === 0x05 && md[k + 3] === 0x00 &&
                md[k + 4] === 0x00 && md[k + 5] === 0x00) {
              const delen = md[k + 2];
              if (delen >= 1 && delen <= 3) {
                const edgeId = readEntityId(md, k + 6, delen);
                edges.set(edgeId, [v1, v2]);
              }
              break;
            }
          }
          break;
        }
      }
    }
  }
  return edges;
}

function extractFaces(md, dv, idSize) {
  const faces = [];
  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] === 0xae && md[i + 1] === 0x0d) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 0 && len < 10000 && i + 6 + len <= md.length) {
        const edgeRefs = [];
        const blockEnd = i + 6 + len;
        for (let j = i + 6; j < blockEnd - 10; j++) {
          if (md[j] === 0xa0 && md[j + 1] === 0x0f && md[j + 3] === 0x00) {
            const a0len = md[j + 2];
            if (a0len < 14 || a0len > 16) continue;
            if (j + 6 < blockEnd && md[j + 6] === 0xa1 && md[j + 7] === 0x0f && md[j + 9] === 0x00) {
              const refIdSize = md[j + 8];
              if (refIdSize < 1 || refIdSize > 3) continue;
              if (j + 12 + refIdSize > blockEnd) continue;
              const edgeId = readEntityId(md, j + 12, refIdSize);
              let reversed = false;
              const a2Offset = j + 12 + refIdSize;
              if (a2Offset + 6 < blockEnd && md[a2Offset] === 0xa2 && md[a2Offset + 1] === 0x0f) {
                reversed = md[a2Offset + 6] === 0x00;
              }
              edgeRefs.push({ id: edgeId, reversed });
            }
          }
        }
        if (edgeRefs.length >= 3) {
          faces.push({ edgeRefs, fileOffset: i });
        }
      }
    }
  }
  return faces;
}

function findDefinitionBlocks(md) {
  const definitions = [];
  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] === 0x7e && md[i + 1] === 0x15) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 0 && len < 300 && i + 6 + len <= md.length) {
        const name = new TextDecoder('utf-8').decode(md.subarray(i + 6, i + 6 + len)).replace(/\0/g, '').trim();
        if (name) definitions.push({ name, offset: i, endOffset: i + 6 + len });
      }
    }
  }
  definitions.sort((a, b) => a.offset - b.offset);

  const geomBlocks = [];
  for (let i = 0; i < md.length - 6; i++) {
    if (md[i] === 0x7c && md[i + 1] === 0x15) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 100 && len < md.length && i + 6 + len <= md.length) {
        geomBlocks.push({ offset: i, dataStart: i + 6, dataEnd: i + 6 + len });
      }
    }
  }

  const blocks = [];
  for (const gb of geomBlocks) {
    let bestDef = null;
    for (const def of definitions) {
      if (def.offset < gb.offset) bestDef = def;
      else break;
    }
    blocks.push({ name: bestDef?.name ?? '__unassigned__', dataStart: gb.dataStart, dataEnd: gb.dataEnd });
  }
  blocks.sort((a, b) => a.dataStart - b.dataStart);
  return blocks;
}

function findInstanceTransforms(md, dv) {
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
      if (!valid || Math.abs(vals[12] - 1.0) > 0.01) continue;

      let defName = '';
      for (let j = i + 110; j < Math.min(i + 3000, md.length - 10); j++) {
        if (md[j] === 0x7e && md[j + 1] === 0x15) {
          const nlen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (nlen > 0 && nlen < 300 && j + 6 + nlen <= md.length) {
            defName = new TextDecoder('utf-8').decode(md.subarray(j + 6, j + 6 + nlen)).replace(/\0/g, '').trim();
            break;
          }
        }
        if (md[j] === 0x66 && md[j + 1] === 0x19) {
          const nextLen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (nextLen === 104) break;
        }
      }
      if (!defName) continue;
      instances.push({ defName, transform: vals, offset: i });
    }
  }
  return instances;
}

function resolveFaceVertices(face, edgeVertices) {
  if (face.edgeRefs.length === 0) return [];
  const edgePairs = [];
  for (const ref of face.edgeRefs) {
    const pair = edgeVertices.get(ref.id);
    if (!pair) continue;
    edgePairs.push(ref.reversed ? [pair[1], pair[0]] : [pair[0], pair[1]]);
  }
  if (edgePairs.length === 0) return [];

  const ordered = [edgePairs[0][0], edgePairs[0][1]];
  const used = new Set([0]);
  for (let step = 0; step < edgePairs.length - 1; step++) {
    const lastVert = ordered[ordered.length - 1];
    let found = false;
    for (let i = 0; i < edgePairs.length; i++) {
      if (used.has(i)) continue;
      const [a, b] = edgePairs[i];
      if (a === lastVert) { ordered.push(b); used.add(i); found = true; break; }
      else if (b === lastVert) { ordered.push(a); used.add(i); found = true; break; }
    }
    if (!found) break;
  }
  if (ordered.length > 1 && ordered[ordered.length - 1] === ordered[0]) ordered.pop();
  return ordered;
}

function findDefinitionForOffset(offset, blocks) {
  let best = null;
  let bestSize = Infinity;
  for (const block of blocks) {
    if (offset >= block.dataStart && offset < block.dataEnd) {
      const size = block.dataEnd - block.dataStart;
      if (size < bestSize) { bestSize = size; best = block; }
    }
  }
  return best;
}

function isIdentityTransform(t) {
  return Math.abs(t[0] - 1) < 1e-6 && Math.abs(t[4] - 1) < 1e-6 && Math.abs(t[8] - 1) < 1e-6 &&
         Math.abs(t[1]) < 1e-6 && Math.abs(t[2]) < 1e-6 && Math.abs(t[3]) < 1e-6 &&
         Math.abs(t[5]) < 1e-6 && Math.abs(t[6]) < 1e-6 && Math.abs(t[7]) < 1e-6 &&
         Math.abs(t[9]) < 0.01 && Math.abs(t[10]) < 0.01 && Math.abs(t[11]) < 0.01;
}

function applyTransform(v, t) {
  return {
    x: v.x * t[0] + v.y * t[3] + v.z * t[6] + t[9],
    y: v.x * t[1] + v.y * t[4] + v.z * t[7] + t[10],
    z: v.x * t[2] + v.y * t[5] + v.z * t[8] + t[11],
  };
}

// ============================================================
// Test runner
// ============================================================

function testSKPFile(filePath) {
  const fileName = path.basename(filePath);
  console.log(`\n${'='.repeat(70)}`);
  console.log(`TESTING: ${fileName}`);
  console.log(`${'='.repeat(70)}`);

  const buf = new Uint8Array(readFileSync(filePath));
  console.log(`File size: ${(buf.length / 1024).toFixed(1)} KB`);

  // 1. Header validation
  if (!validateSKPHeader(buf)) {
    console.log(`  FAIL: Invalid SKP header`);
    return { file: fileName, status: 'FAIL', reason: 'Invalid header' };
  }
  const { major, versionStr } = parseSKPVersion(buf);
  console.log(`  PASS: Valid SKP header, version ${versionStr} (major=${major})`);

  // 2. Find ZIP offset
  const zipOff = findZipOffset(buf);
  if (zipOff === -1) {
    console.log(`  SKIP: No ZIP archive (pre-2021 format)`);
    return { file: fileName, status: 'SKIP', reason: `Pre-2021 format (${versionStr})` };
  }
  console.log(`  ZIP at offset ${zipOff}`);

  // 3. ZIP extraction
  let zipFiles;
  try {
    zipFiles = unzipSync(buf.subarray(zipOff));
  } catch (err) {
    console.log(`  FAIL: ZIP extraction failed: ${err.message}`);
    return { file: fileName, status: 'FAIL', reason: 'ZIP extraction failed' };
  }
  const zipFileNames = Object.keys(zipFiles);
  console.log(`  PASS: ZIP extracted (${zipFileNames.length} files: ${zipFileNames.slice(0, 5).join(', ')}${zipFileNames.length > 5 ? '...' : ''})`);

  const md = zipFiles['model.dat'];
  if (!md) {
    console.log(`  FAIL: No model.dat in ZIP`);
    return { file: fileName, status: 'FAIL', reason: 'No model.dat' };
  }
  console.log(`  model.dat: ${(md.length / 1024).toFixed(1)} KB`);

  const dv = new DataView(md.buffer, md.byteOffset, md.byteLength);
  const idSize = detectEntityIdSize(md);
  console.log(`  Entity ID size: ${idSize} bytes`);

  // 4. Extract geometry
  const vertices = extractVertices(md, dv, idSize);
  const edges = extractEdges(md, idSize);
  const faces = extractFaces(md, dv, idSize);
  console.log(`  Vertices: ${vertices.size}, Edges: ${edges.size}, Faces: ${faces.length}`);

  if (faces.length === 0) {
    console.log(`  WARN: No faces found - file may be empty or use different format version`);
    return { file: fileName, status: 'WARN', reason: 'No faces', vertices: vertices.size, edges: edges.size, faces: 0 };
  }

  // 4. Definition blocks
  const defBlocks = findDefinitionBlocks(md);
  const defNames = new Set(defBlocks.map(b => b.name));
  console.log(`  Definition blocks: ${defBlocks.length} (${defNames.size} unique names)`);

  // 5. Instance transforms
  const instances = findInstanceTransforms(md, dv);
  const instNames = new Set(instances.map(i => i.defName));
  console.log(`  Instance transforms: ${instances.length} (${instNames.size} unique defs)`);

  // 6. Resolve faces
  let resolved = 0;
  const resolvedFaces = [];
  for (const face of faces) {
    const vertIds = resolveFaceVertices(face, edges);
    if (vertIds.length < 3) continue;
    const verts = [];
    let allFound = true;
    for (const vid of vertIds) {
      const pos = vertices.get(vid);
      if (!pos) { allFound = false; break; }
      verts.push(pos);
    }
    if (!allFound || verts.length < 3) continue;
    resolved++;
    resolvedFaces.push({ face, verts });
  }
  console.log(`  Resolved faces: ${resolved}/${faces.length} (${(resolved/faces.length*100).toFixed(1)}%)`);

  // 7. Classify faces by definition
  const rootFaces = [];
  const defFacesMap = new Map();
  for (const rf of resolvedFaces) {
    const block = findDefinitionForOffset(rf.face.fileOffset, defBlocks);
    if (!block || block.name === '__root__') {
      rootFaces.push(rf);
    } else {
      if (!defFacesMap.has(block.name)) defFacesMap.set(block.name, []);
      defFacesMap.get(block.name).push(rf);
    }
  }

  const instancesByDef = new Map();
  for (const inst of instances) {
    if (!instancesByDef.has(inst.defName)) instancesByDef.set(inst.defName, []);
    instancesByDef.get(inst.defName).push(inst);
  }

  // 8. Spatial filtering (matching SKPLoader.ts logic)
  const identityDefEntries = [];
  const instancedDefEntries = [];
  for (const [defName, defFaces] of defFacesMap) {
    const insts = instancesByDef.get(defName);
    if (insts && insts.length > 0) instancedDefEntries.push([defName, defFaces]);
    else identityDefEntries.push([defName, defFaces]);
  }
  identityDefEntries.sort((a, b) => b[1].length - a[1].length);

  function computeVertBBox(faceSets) {
    let minX = Infinity, minY = Infinity, minZ = Infinity;
    let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
    for (const faceSet of faceSets) {
      for (const rf of faceSet) {
        for (const v of rf.verts) {
          if (v.x < minX) minX = v.x;
          if (v.y < minY) minY = v.y;
          if (v.z < minZ) minZ = v.z;
          if (v.x > maxX) maxX = v.x;
          if (v.y > maxY) maxY = v.y;
          if (v.z > maxZ) maxZ = v.z;
        }
      }
    }
    return { minX, minY, minZ, maxX, maxY, maxZ };
  }

  const refSets = [rootFaces];
  if (identityDefEntries.length > 0) refSets.push(identityDefEntries[0][1]);
  const refBBox = computeVertBBox(refSets);

  const refDiagX = refBBox.maxX - refBBox.minX;
  const refDiagY = refBBox.maxY - refBBox.minY;
  const refDiagZ = refBBox.maxZ - refBBox.minZ;
  const marginX = Math.max(refDiagX, 1000);
  const marginY = Math.max(refDiagY, 1000);
  const marginZ = Math.max(refDiagZ, 1000);

  let identityRendered = 0;
  let identitySkipped = 0;
  let identityFaces = 0;
  let skippedFaces = 0;

  for (const [defName, defFaces] of identityDefEntries) {
    const isRef = identityDefEntries[0][0] === defName;
    let cx = 0, cy = 0, cz = 0, n = 0;
    for (const rf of defFaces) {
      for (const v of rf.verts) { cx += v.x; cy += v.y; cz += v.z; n++; }
    }
    if (n > 0) { cx /= n; cy /= n; cz /= n; }

    const inside = isRef || (
      cx >= refBBox.minX - marginX && cx <= refBBox.maxX + marginX &&
      cy >= refBBox.minY - marginY && cy <= refBBox.maxY + marginY &&
      cz >= refBBox.minZ - marginZ && cz <= refBBox.maxZ + marginZ
    );

    if (inside) {
      identityRendered++;
      identityFaces += defFaces.length;
    } else {
      identitySkipped++;
      skippedFaces += defFaces.length;
      console.log(`    SKIP outlier: "${defName}" (${defFaces.length} faces, center: ${(cx * INCHES_TO_METERS).toFixed(1)}, ${(cz * INCHES_TO_METERS).toFixed(1)}, ${(-cy * INCHES_TO_METERS).toFixed(1)} m)`);
    }
  }

  // Count instanced faces
  let instancedFaceCount = 0;
  for (const [defName, defFaces] of instancedDefEntries) {
    const insts = instancesByDef.get(defName);
    instancedFaceCount += defFaces.length * insts.length;
  }

  const totalRendered = rootFaces.length + identityFaces + instancedFaceCount;

  console.log(`\n  RESULTS:`);
  console.log(`    Root faces: ${rootFaces.length}`);
  console.log(`    Identity defs rendered: ${identityRendered} (${identityFaces} faces)`);
  console.log(`    Identity defs skipped (outliers): ${identitySkipped} (${skippedFaces} faces)`);
  console.log(`    Instanced defs: ${instancedDefEntries.length} (${instancedFaceCount} world-space faces)`);
  console.log(`    Total rendered faces: ${totalRendered}`);
  console.log(`    Coverage: ${(totalRendered / resolved * 100).toFixed(1)}%`);

  // Compute final bounding box (in meters)
  const allRenderedVerts = [];
  for (const rf of rootFaces) {
    for (const v of rf.verts) allRenderedVerts.push(v);
  }
  for (const [defName, defFaces] of identityDefEntries) {
    const isRef = identityDefEntries[0][0] === defName;
    let cx = 0, cy = 0, cz = 0, n = 0;
    for (const rf of defFaces) {
      for (const v of rf.verts) { cx += v.x; cy += v.y; cz += v.z; n++; }
    }
    if (n > 0) { cx /= n; cy /= n; cz /= n; }
    const inside = isRef || (
      cx >= refBBox.minX - marginX && cx <= refBBox.maxX + marginX &&
      cy >= refBBox.minY - marginY && cy <= refBBox.maxY + marginY &&
      cz >= refBBox.minZ - marginZ && cz <= refBBox.maxZ + marginZ
    );
    if (inside) {
      for (const rf of defFaces) {
        for (const v of rf.verts) allRenderedVerts.push(v);
      }
    }
  }
  for (const [defName, defFaces] of instancedDefEntries) {
    const insts = instancesByDef.get(defName);
    for (const inst of insts) {
      for (const rf of defFaces) {
        for (const v of rf.verts) {
          allRenderedVerts.push(isIdentityTransform(inst.transform) ? v : applyTransform(v, inst.transform));
        }
      }
    }
  }

  if (allRenderedVerts.length > 0) {
    let minX = Infinity, minY = Infinity, minZ = Infinity;
    let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
    for (const v of allRenderedVerts) {
      // Convert to Three.js (X,Z,-Y) in meters
      const x = v.x * INCHES_TO_METERS;
      const y = v.z * INCHES_TO_METERS;
      const z = -v.y * INCHES_TO_METERS;
      if (x < minX) minX = x; if (x > maxX) maxX = x;
      if (y < minY) minY = y; if (y > maxY) maxY = y;
      if (z < minZ) minZ = z; if (z > maxZ) maxZ = z;
    }
    const sizeX = maxX - minX;
    const sizeY = maxY - minY;
    const sizeZ = maxZ - minZ;
    console.log(`    Scene bbox: X[${minX.toFixed(2)}, ${maxX.toFixed(2)}] Y[${minY.toFixed(2)}, ${maxY.toFixed(2)}] Z[${minZ.toFixed(2)}, ${maxZ.toFixed(2)}]`);
    console.log(`    Scene size: ${sizeX.toFixed(2)} x ${sizeY.toFixed(2)} x ${sizeZ.toFixed(2)} meters`);
  }

  return {
    file: fileName,
    status: totalRendered > 0 ? 'PASS' : 'WARN',
    vertices: vertices.size,
    edges: edges.size,
    faces: faces.length,
    resolved,
    rootFaces: rootFaces.length,
    identityDefs: identityRendered,
    skippedOutliers: identitySkipped,
    instancedDefs: instancedDefEntries.length,
    totalRendered,
    coverage: (totalRendered / Math.max(resolved, 1) * 100).toFixed(1) + '%',
  };
}

// ============================================================
// Run all tests
// ============================================================

const testFiles = [];

// Main test file
if (readFileSync('test-kavel.skp', { flag: 'r' })) {
  testFiles.push('test-kavel.skp');
}

// Downloaded test files
try {
  const modelDir = 'test-models';
  const files = readdirSync(modelDir);
  for (const f of files) {
    if (f.toLowerCase().endsWith('.skp')) {
      testFiles.push(path.join(modelDir, f));
    }
  }
} catch {}

console.log(`\nSKP LOADER TEST PROTOCOL`);
console.log(`Testing ${testFiles.length} files...\n`);

const results = [];
for (const file of testFiles) {
  try {
    results.push(testSKPFile(file));
  } catch (err) {
    console.log(`\n  ERROR: ${err.message}`);
    results.push({ file: path.basename(file), status: 'ERROR', reason: err.message });
  }
}

console.log(`\n${'='.repeat(70)}`);
console.log('SUMMARY');
console.log(`${'='.repeat(70)}`);
console.log(`\n${'File'.padEnd(40)} ${'Status'.padEnd(8)} ${'Faces'.padEnd(8)} ${'Rendered'.padEnd(10)} ${'Coverage'}`);
console.log('-'.repeat(80));
for (const r of results) {
  const faces = r.faces ?? 0;
  const rendered = r.totalRendered ?? 0;
  const coverage = r.coverage ?? 'N/A';
  console.log(`${r.file.padEnd(40)} ${r.status.padEnd(8)} ${String(faces).padEnd(8)} ${String(rendered).padEnd(10)} ${coverage}`);
}

const passed = results.filter(r => r.status === 'PASS').length;
const warned = results.filter(r => r.status === 'WARN').length;
const failed = results.filter(r => r.status === 'FAIL' || r.status === 'ERROR').length;
console.log(`\n${passed} PASS, ${warned} WARN, ${failed} FAIL/ERROR out of ${results.length} files`);
