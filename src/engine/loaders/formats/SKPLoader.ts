/**
 * SketchUp SKP File Loader
 *
 * Reverse-engineered parser for SketchUp files.
 * Supports: v2024+ (v24-v26 format with 3-byte entity IDs)
 *
 * SKP files are: 65-byte header + ZIP archive containing model.dat (binary geometry)
 * and materials/ folder with material.xml + texture files.
 *
 * Binary format: custom TLV (Tag-Length-Value) with 2-byte LE tags + 4-byte LE lengths.
 *
 * Key tags:
 *   c5_09 len=24  → vertex position (3 doubles: x, y, z in inches)
 *   b9_0b len=3   → edge start vertex reference (3-byte entity ID)
 *   ba_0b len=3   → edge end vertex reference (3-byte entity ID)
 *   ad_0d len=32  → face plane equation (4 doubles: nx, ny, nz, d)
 *   ae_0d          → face edge reference list
 *   a0_0f + a1_0f → entity reference (3-byte ID) with direction flag
 *   d1_07 len=3   → material entity reference (on faces)
 *   cc_32          → material name string (within material entities)
 *   7c_15          → geometry container block within definitions
 *   66_19 len=104  → component instance transform (13 doubles: 3x3 matrix + translation + 1.0)
 *   7e_15          → definition name string
 */

import { unzipSync } from 'fflate';
import * as THREE from 'three';
import { Earcut } from 'three/src/extras/Earcut.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

// ============================================================================
// Types
// ============================================================================

interface Vec3 {
  x: number;
  y: number;
  z: number;
}

interface SKPFace {
  edgeRefs: { id: number; reversed: boolean }[];
  plane: { nx: number; ny: number; nz: number; d: number } | null;
  materialRef: number | null;
  fileOffset: number;
}

interface SKPMaterialInfo {
  name: string;
  colorR: number;
  colorG: number;
  colorB: number;
  opacity: number;
  textureFile: string | null;
}

interface DefinitionBlock {
  name: string;
  nameOffset: number; // Position of the 7e_15 name tag in the binary
  dataStart: number;
  dataEnd: number;
}

interface InstanceTransform {
  defName: string;
  transform: number[]; // 13 doubles: [m0..m8, tx, ty, tz, 1.0]
  offset: number;
  parentDef?: string | null; // Set if nested inside another definition
  isDefinitionHeader?: boolean; // True = template at origin, not a real placement
}

const INCHES_TO_METERS = 0.0254;

// ============================================================================
// Main loader
// ============================================================================

export async function loadSKP(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  const buf = new Uint8Array(data);

  onProgress?.(0.05, 'SKP header controleren...');

  if (!validateSKPHeader(buf)) {
    throw new Error(
      `Ongeldig SKP bestand: ${fileName}\n` +
      `Het bestand heeft geen geldig SketchUp header.`
    );
  }

  // Check version and find ZIP offset
  const { major, versionStr } = parseSKPVersion(buf);
  console.log(`SKP: Detected format version ${versionStr} (major=${major})`);

  onProgress?.(0.1, 'ZIP archief uitpakken...');

  // Find ZIP magic (PK\x03\x04) — offset varies by version string length
  let zipOffset = -1;
  for (let i = 40; i < Math.min(buf.length, 200); i++) {
    if (buf[i] === 0x50 && buf[i + 1] === 0x4B && buf[i + 2] === 0x03 && buf[i + 3] === 0x04) {
      zipOffset = i;
      break;
    }
  }

  if (zipOffset === -1) {
    if (major > 0 && major < 21) {
      throw new Error(
        `SketchUp formaat ${versionStr} wordt niet ondersteund.\n` +
        `Dit is een ouder binair formaat (pre-2021). Open het bestand in SketchUp 2021 of nieuwer en sla het opnieuw op.`
      );
    }
    throw new Error(
      `Geen ZIP archief gevonden in SKP bestand: ${fileName}\n` +
      `Het bestand is mogelijk beschadigd of gebruikt een onbekend formaat (${versionStr}).`
    );
  }

  console.log(`SKP: ZIP archive at offset ${zipOffset}`);
  const zipData = buf.subarray(zipOffset);
  let zipFiles: Record<string, Uint8Array>;
  try {
    zipFiles = unzipSync(zipData);
  } catch (err) {
    throw new Error(
      `Kan SKP ZIP niet uitpakken: ${fileName}\n` +
      `${err instanceof Error ? err.message : String(err)}`
    );
  }

  const modelDat = zipFiles['model.dat'];
  if (!modelDat) {
    throw new Error(
      `Geen model.dat gevonden in SKP bestand: ${fileName}\n` +
      `Het bestand is mogelijk beschadigd.`
    );
  }

  const dv = new DataView(modelDat.buffer, modelDat.byteOffset, modelDat.byteLength);
  const md = modelDat;

  // Auto-detect entity ID size: scan for first de_05 tag and check its length
  const entityIdSize = detectEntityIdSize(md);
  console.log(`SKP: Entity ID size: ${entityIdSize} bytes`);

  // Extract raw geometry data
  onProgress?.(0.15, 'Vertices scannen...');
  const vertexPositions = extractVertices(md, dv, entityIdSize);
  onProgress?.(0.25, `${vertexPositions.size} vertices gevonden...`);

  const edgeVertices = extractEdges(md, entityIdSize);
  onProgress?.(0.35, `${edgeVertices.size} edges gevonden...`);

  const faces = extractFaces(md, dv, entityIdSize);
  onProgress?.(0.45, `${faces.length} faces gevonden...`);

  // Extract material data
  onProgress?.(0.5, 'Materialen laden...');
  const materialEntityMap = extractMaterialEntityMap(md, entityIdSize);
  const materialColors = parseMaterialsFromZip(zipFiles);
  onProgress?.(0.55, `${materialColors.size} materialen geladen...`);

  // Extract scene hierarchy
  onProgress?.(0.58, 'Definitieblokken scannen...');
  const definitionBlocks = findDefinitionBlocks(md);
  onProgress?.(0.6, `${definitionBlocks.length} definities gevonden...`);

  const instanceTransforms = findInstanceTransforms(md, dv);
  onProgress?.(0.63, `${instanceTransforms.length} instanties gevonden...`);

  // Build Three.js scene with definition/instance transforms
  onProgress?.(0.65, 'Scene opbouwen...');
  const result = buildThreeJSScene(
    vertexPositions, edgeVertices, faces,
    materialEntityMap, materialColors, zipFiles,
    definitionBlocks, instanceTransforms,
    onProgress
  );

  onProgress?.(1.0, 'Klaar');
  return result;
}

// ============================================================================
// Header validation
// ============================================================================

function validateSKPHeader(buf: Uint8Array): boolean {
  if (buf.length < 65) return false;
  if (buf[0] !== 0xFF || buf[1] !== 0xFE || buf[2] !== 0xFF) return false;
  const marker = String.fromCharCode(buf[4], buf[6], buf[8], buf[10], buf[12], buf[14], buf[16], buf[18]);
  return marker === 'SketchUp';
}

/**
 * Parse the SKP version string from the header.
 * Header format: FF FE FF 0E "SketchUp Model" FF FE FF NN "version" VFF...
 * Version string is like "{26.1.0}" or "{21.0.0}" etc.
 */
function parseSKPVersion(buf: Uint8Array): { major: number; versionStr: string } {
  // Find the version string after "SketchUp Model" + second FF FE FF marker
  // The second marker is at offset 31 typically, followed by length byte then UTF-16LE string
  try {
    // Look for second FF FE FF
    for (let i = 20; i < Math.min(60, buf.length - 10); i++) {
      if (buf[i] === 0xFF && buf[i + 1] === 0xFE && buf[i + 2] === 0xFF) {
        const strLen = buf[i + 3]; // number of UTF-16 chars
        if (strLen > 0 && strLen < 20 && i + 4 + strLen * 2 <= buf.length) {
          let versionStr = '';
          for (let j = 0; j < strLen; j++) {
            versionStr += String.fromCharCode(buf[i + 4 + j * 2]);
          }
          // Parse major version from "{XX.Y.Z}"
          const match = versionStr.match(/\{(\d+)\./);
          if (match) {
            return { major: parseInt(match[1]), versionStr };
          }
        }
      }
    }
  } catch {
    // Fall through
  }
  return { major: 0, versionStr: 'unknown' };
}

// ============================================================================
// Entity ID size detection
// ============================================================================

/**
 * Auto-detect entity ID size by looking at de_05 tags near c5_09 vertex tags.
 * v21 format uses 2-byte IDs, v24+ uses 3-byte IDs.
 * We check de_05 tags that appear right before vertex position tags (c5_09 len=24)
 * since those are guaranteed to be entity IDs.
 */
function detectEntityIdSize(md: Uint8Array): number {
  let count2 = 0;
  let count3 = 0;
  const limit = Math.min(md.length - 30, 500000); // check first 500KB

  for (let i = 0; i < limit; i++) {
    // Find c5_09 vertex tag (len=24 = 0x18)
    if (md[i] === 0xc5 && md[i + 1] === 0x09 &&
        md[i + 2] === 0x18 && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      // Look backward for the de_05 entity tag
      for (let j = i - 20; j < i; j++) {
        if (j >= 0 && md[j] === 0xde && md[j + 1] === 0x05) {
          const len = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (len === 2) count2++;
          else if (len === 3) count3++;
          break;
        }
      }
      // Stop after enough samples
      if (count2 + count3 >= 20) break;
    }
  }

  const result = count3 > count2 ? 3 : count2 > 0 ? 2 : 3;
  console.log(`SKP: Entity ID size detection: ${count2} x 2-byte, ${count3} x 3-byte → using ${result}`);
  return result;
}

/** Read an entity ID of variable size (2 or 3 bytes, big-endian). */
function readEntityId(md: Uint8Array, offset: number, size: number): number {
  if (size === 3) {
    return (md[offset] << 16) | (md[offset + 1] << 8) | md[offset + 2];
  }
  return (md[offset] << 8) | md[offset + 1];
}

// ============================================================================
// Vertex extraction
// ============================================================================

function extractVertices(md: Uint8Array, dv: DataView, _idSize: number): Map<number, Vec3> {
  const positions = new Map<number, Vec3>();

  for (let i = 0; i < md.length - 30; i++) {
    if (md[i] === 0xc5 && md[i + 1] === 0x09 &&
        md[i + 2] === 0x18 && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      // Find de_05 entity ID tag before this vertex position — accept variable ID sizes
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

// ============================================================================
// Edge extraction
// ============================================================================

function extractEdges(md: Uint8Array, idSize: number): Map<number, [number, number]> {
  const edges = new Map<number, [number, number]>();

  for (let i = 0; i < md.length - 20; i++) {
    // b9_0b: edge start vertex ref — accept variable ID sizes
    if (md[i] === 0xb9 && md[i + 1] === 0x0b && md[i + 3] === 0x00 &&
        md[i + 4] === 0x00 && md[i + 5] === 0x00) {
      const b9len = md[i + 2];
      if (b9len < 1 || b9len > 3) continue;
      const v1 = readEntityId(md, i + 6, b9len);
      const searchStart = i + 6 + b9len;

      for (let j = searchStart; j < Math.min(searchStart + 12, md.length - 9); j++) {
        // ba_0b: edge end vertex ref
        if (md[j] === 0xba && md[j + 1] === 0x0b && md[j + 3] === 0x00 &&
            md[j + 4] === 0x00 && md[j + 5] === 0x00) {
          const balen = md[j + 2];
          if (balen < 1 || balen > 3) break;
          const v2 = readEntityId(md, j + 6, balen);

          // Find the de_05 entity ID for this edge
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

// ============================================================================
// Face extraction (with material references)
// ============================================================================

function extractFaces(md: Uint8Array, dv: DataView, idSize: number): SKPFace[] {
  const faces: SKPFace[] = [];

  // Edge ref structure: a0_0f(N) { a1_0f(M) { edgeId[M] } a2_0f(1) { reversed } }
  // N = 13 + M where M = entity ID size (1, 2, or 3 bytes)
  // Entity ID size can vary WITHIN a single file (compact encoding for small IDs)

  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] === 0xae && md[i + 1] === 0x0d) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 0 && len < 10000 && i + 6 + len <= md.length) {
        const edgeRefs: { id: number; reversed: boolean }[] = [];

        const blockEnd = i + 6 + len;
        for (let j = i + 6; j < blockEnd - 10; j++) {
          // Match a0_0f with any valid edge ref length (14, 15, or 16)
          if (md[j] === 0xa0 && md[j + 1] === 0x0f && md[j + 3] === 0x00) {
            const a0len = md[j + 2];
            if (a0len < 14 || a0len > 16) continue;

            // Read a1_0f: entity ID reference with variable length
            if (j + 6 < blockEnd && md[j + 6] === 0xa1 && md[j + 7] === 0x0f && md[j + 9] === 0x00) {
              const refIdSize = md[j + 8]; // 1, 2, or 3
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
          const searchStart = Math.max(0, i - 150);
          let plane: SKPFace['plane'] = null;
          let materialRef: number | null = null;

          for (let j = searchStart; j < i; j++) {
            if (md[j] === 0xad && md[j + 1] === 0x0d &&
                md[j + 2] === 0x20 && md[j + 3] === 0x00 &&
                md[j + 4] === 0x00 && md[j + 5] === 0x00) {
              plane = {
                nx: dv.getFloat64(j + 6, true),
                ny: dv.getFloat64(j + 14, true),
                nz: dv.getFloat64(j + 22, true),
                d: dv.getFloat64(j + 30, true),
              };
            }

            // Material ref: also accept variable ID sizes
            if (md[j] === 0xd1 && md[j + 1] === 0x07 && md[j + 3] === 0x00 &&
                md[j + 4] === 0x00 && md[j + 5] === 0x00) {
              const matIdSize = md[j + 2];
              if (matIdSize >= 1 && matIdSize <= 3) {
                materialRef = readEntityId(md, j + 6, matIdSize);
              }
            }
          }

          faces.push({ edgeRefs, plane, materialRef, fileOffset: i });
        }
      }
    }
  }

  return faces;
}

// ============================================================================
// Material extraction from model.dat binary
// ============================================================================

function extractMaterialEntityMap(md: Uint8Array, _idSize: number): Map<number, string> {
  const map = new Map<number, string>();

  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] === 0xcc && md[i + 1] === 0x32) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 0 && len < 300 && i + 6 + len <= md.length) {
        const name = new TextDecoder('utf-8')
          .decode(md.subarray(i + 6, i + 6 + len))
          .replace(/\0/g, '')
          .trim();
        if (!name) continue;

        for (let j = i - 1; j >= Math.max(0, i - 500); j--) {
          if (md[j] === 0xde && md[j + 1] === 0x05 && md[j + 3] === 0x00 &&
              md[j + 4] === 0x00 && md[j + 5] === 0x00) {
            const delen = md[j + 2];
            if (delen >= 1 && delen <= 3) {
              const id = readEntityId(md, j + 6, delen);
              map.set(id, name);
            }
            break;
          }
        }
      }
    }
  }

  return map;
}

// ============================================================================
// Material parsing from ZIP (material.xml files)
// ============================================================================

function parseMaterialsFromZip(zipFiles: Record<string, Uint8Array>): Map<string, SKPMaterialInfo> {
  const materials = new Map<string, SKPMaterialInfo>();

  for (const [path, data] of Object.entries(zipFiles)) {
    if (!path.match(/^materials\/.*\/material\.xml$/i)) continue;

    const xml = new TextDecoder('utf-8').decode(data);

    const nameMatch = xml.match(/name="([^"]+)"/);
    if (!nameMatch) continue;
    const name = decodeXmlEntities(nameMatch[1]);

    const rMatch = xml.match(/colorRed="(\d+)"/);
    const gMatch = xml.match(/colorGreen="(\d+)"/);
    const bMatch = xml.match(/colorBlue="(\d+)"/);
    const transMatch = xml.match(/\btrans="([^"]+)"/);
    const useTransMatch = xml.match(/useTrans="([^"]+)"/);

    const colorR = rMatch ? parseInt(rMatch[1]) : 200;
    const colorG = gMatch ? parseInt(gMatch[1]) : 200;
    const colorB = bMatch ? parseInt(bMatch[1]) : 200;

    let opacity = 1;
    if (useTransMatch && useTransMatch[1] === '1' && transMatch) {
      opacity = 1 - parseFloat(transMatch[1]);
    }

    let textureFile: string | null = null;
    const texPathMatch = xml.match(/path="\.\/([^"]+)"/);
    if (texPathMatch) {
      const matDir = path.substring(0, path.lastIndexOf('/'));
      const candidate = `${matDir}/${texPathMatch[1]}`;
      if (zipFiles[candidate]) {
        textureFile = candidate;
      }
    }

    materials.set(name, { name, colorR, colorG, colorB, opacity, textureFile });

    const folderName = path.split('/').slice(-2, -1)[0];
    if (folderName && folderName !== name) {
      materials.set(folderName, { name, colorR, colorG, colorB, opacity, textureFile });
    }
  }

  return materials;
}

function decodeXmlEntities(s: string): string {
  return s
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'");
}

// ============================================================================
// Definition block extraction (7c_15 = geometry containers within definitions)
//
// Structure: 7e_15("name") → 7f_15(0) → ... → 7c_15(geometry) → 7c_15(geometry)
// Each 7c_15 block contains faces, edges, and vertices for that definition.
// ============================================================================

function findDefinitionBlocks(md: Uint8Array): DefinitionBlock[] {
  // Step 1: Find all definition names (7e_15)
  const definitions: { name: string; offset: number; endOffset: number }[] = [];
  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] === 0x7e && md[i + 1] === 0x15) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 0 && len < 300 && i + 6 + len <= md.length) {
        const name = new TextDecoder('utf-8')
          .decode(md.subarray(i + 6, i + 6 + len))
          .replace(/\0/g, '')
          .trim();
        if (name) {
          definitions.push({ name, offset: i, endOffset: i + 6 + len });
        }
      }
    }
  }
  definitions.sort((a, b) => a.offset - b.offset);

  // Step 2: Find all 7c_15 geometry blocks
  const geomBlocks: { offset: number; dataStart: number; dataEnd: number }[] = [];
  for (let i = 0; i < md.length - 6; i++) {
    if (md[i] === 0x7c && md[i + 1] === 0x15) {
      const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
      if (len > 100 && len < md.length && i + 6 + len <= md.length) {
        geomBlocks.push({ offset: i, dataStart: i + 6, dataEnd: i + 6 + len });
      }
    }
  }

  // Step 3: Associate each 7c_15 block with its preceding definition name.
  // A 7c_15 block belongs to the last 7e_15 definition that appeared before it.
  const blocks: DefinitionBlock[] = [];
  for (const gb of geomBlocks) {
    // Find the last definition before this geometry block
    let bestDef: typeof definitions[0] | null = null;
    for (const def of definitions) {
      if (def.offset < gb.offset) {
        bestDef = def;
      } else {
        break; // definitions are sorted by offset
      }
    }

    blocks.push({
      name: bestDef?.name ?? '__unassigned__',
      nameOffset: bestDef?.offset ?? gb.offset,
      dataStart: gb.dataStart,
      dataEnd: gb.dataEnd,
    });
  }

  // Sort by offset
  blocks.sort((a, b) => a.dataStart - b.dataStart);

  // Log summary
  const byName = new Map<string, number>();
  for (const b of blocks) {
    byName.set(b.name, (byName.get(b.name) ?? 0) + 1);
  }
  console.log(`SKP: Found ${blocks.length} geometry blocks for ${byName.size} definitions:`);
  const sortedNames = [...byName.entries()].sort((a, b) => b[1] - a[1]);
  for (const [name, count] of sortedNames.slice(0, 15)) {
    console.log(`  "${name}": ${count} geometry blocks`);
  }
  if (sortedNames.length > 15) console.log(`  ... and ${sortedNames.length - 15} more`);

  return blocks;
}

// ============================================================================
// Entity ID helpers for instance → definition resolution
// ============================================================================

/** Convert a byte sequence to a hex string ID. */
function hexId(md: Uint8Array, offset: number, len: number): string {
  let s = '';
  for (let i = 0; i < len; i++) {
    s += md[offset + i].toString(16).padStart(2, '0');
  }
  return s;
}

/**
 * Build a map of child entity IDs → definition names.
 * For each 7e_15 definition name, scans backward for 8e_13 tags (child entity ID lists)
 * and forward into 85_15 blocks for de_05 entity IDs.
 * These IDs are referenced by de_05 tags in instance data (64_19/4c_1d blocks).
 */
function buildChildIdToDefNameMap(md: Uint8Array): Map<string, string> {
  const map = new Map<string, string>();

  for (let i = 0; i < md.length - 10; i++) {
    if (md[i] !== 0x7e || md[i + 1] !== 0x15) continue;
    const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
    if (len <= 0 || len >= 300 || i + 6 + len > md.length) continue;
    const name = new TextDecoder('utf-8')
      .decode(md.subarray(i + 6, i + 6 + len))
      .replace(/\0/g, '').trim();
    if (!name) continue;

    // Scan backward for 8e_13 child entity ID list
    for (let j = i - 1; j >= Math.max(0, i - 300); j--) {
      if (md[j] === 0x8e && md[j + 1] === 0x13) {
        const l = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
        if (l > 0 && l < 500 && j + 6 + l <= md.length) {
          // Parse: prefix byte (= size in bytes) followed by entity ID bytes
          let p = 0;
          while (p < l) {
            const sz = md[j + 6 + p];
            if (sz >= 1 && sz <= 4 && p + 1 + sz <= l) {
              map.set(hexId(md, j + 6 + p + 1, sz), name);
              p += 1 + sz;
            } else {
              break;
            }
          }
        }
        break;
      }
      if ((md[j] === 0x66 && md[j + 1] === 0x19) || (md[j] === 0x7c && md[j + 1] === 0x15)) break;
    }

    // Scan forward into 85_15 for additional de_05 entity IDs
    const afterName = i + 6 + len;
    for (let j = afterName; j < Math.min(afterName + 400, md.length - 6); j++) {
      if (md[j] === 0x85 && md[j + 1] === 0x15) {
        const l = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
        if (l > 0 && l < 10000 && j + 6 + l <= md.length) {
          for (let k = 0; k < l - 6; k++) {
            if (md[j + 6 + k] === 0xde && md[j + 6 + k + 1] === 0x05) {
              const el = md[j + 6 + k + 2] | (md[j + 6 + k + 3] << 8) |
                         (md[j + 6 + k + 4] << 16) | (md[j + 6 + k + 5] << 24);
              if (el >= 1 && el <= 4 && k + 6 + el <= l) {
                const eid = hexId(md, j + 6 + k + 6, el);
                if (!map.has(eid)) map.set(eid, name);
              }
            }
          }
        }
        break;
      }
    }
  }

  return map;
}

/**
 * Extract de_05 entity ID from instance reference data (64_19 or 4c_1d block).
 * Returns the hex ID string, or null if not found.
 */
function extractDe05FromInstance(md: Uint8Array, scanPos: number): string | null {
  // Try: if next tag is 64_19 or 4c_1d, parse its data for de_05
  if (scanPos + 6 <= md.length) {
    const t0 = md[scanPos], t1 = md[scanPos + 1];
    if ((t0 === 0x64 && t1 === 0x19) || (t0 === 0x4c && t1 === 0x1d)) {
      const dataLen = md[scanPos + 2] | (md[scanPos + 3] << 8) |
                      (md[scanPos + 4] << 16) | (md[scanPos + 5] << 24);
      if (dataLen > 0 && dataLen < 10000 && scanPos + 6 + dataLen <= md.length) {
        const end = scanPos + 6 + dataLen;
        for (let k = scanPos + 6; k < end - 6; k++) {
          if (md[k] === 0xde && md[k + 1] === 0x05) {
            const el = md[k + 2] | (md[k + 3] << 8) | (md[k + 4] << 16) | (md[k + 5] << 24);
            if (el >= 1 && el <= 4 && k + 6 + el <= end) {
              return hexId(md, k + 6, el);
            }
          }
        }
      }
    }
  }

  // Fallback: scan forward up to 600 bytes for de_05
  const searchEnd = Math.min(scanPos + 600, md.length - 6);
  for (let j = scanPos; j < searchEnd; j++) {
    if (md[j] === 0xde && md[j + 1] === 0x05) {
      const el = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
      if (el >= 1 && el <= 4 && j + 6 + el <= md.length) {
        return hexId(md, j + 6, el);
      }
    }
    // Stop at next 66_19 instance
    if (md[j] === 0x66 && md[j + 1] === 0x19) {
      const nl = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
      if (nl === 104) break;
    }
  }

  return null;
}

// ============================================================================
// Instance transform extraction (66_19 tags = component placement transforms)
//
// Two-phase approach:
// Phase 1: Scan all 66_19 instances, read 67_19 entity ID, classify as
//          definition-header (9e_13) or reference instance (64_19/4c_1d).
// Phase 2: Resolve names using entity ID mapping and de_05 → child ID mapping.
// ============================================================================

function findInstanceTransforms(md: Uint8Array, dv: DataView): InstanceTransform[] {
  // Phase 1: Build child entity ID → definition name map from 8e_13 tags in definition headers
  const childIdToDefName = buildChildIdToDefNameMap(md);
  console.log(`SKP: Built child-ID → definition map: ${childIdToDefName.size} entries`);

  // Phase 2: Scan all 66_19 instances, read 67_19 entity ID, classify each
  const rawInstances: {
    offset: number;
    entityId: string | null;
    transform: number[];
    isDefHeader: boolean;
    defName: string | null;
    de05Id: string | null;
  }[] = [];

  for (let i = 0; i < md.length - 110; i++) {
    if (md[i] !== 0x66 || md[i + 1] !== 0x19) continue;
    const len = md[i + 2] | (md[i + 3] << 8) | (md[i + 4] << 16) | (md[i + 5] << 24);
    if (len !== 104) continue;

    // Read 13 float64s
    const vals: number[] = [];
    let valid = true;
    for (let k = 0; k < 13; k++) {
      const v = dv.getFloat64(i + 6 + k * 8, true);
      if (!isFinite(v)) { valid = false; break; }
      vals.push(v);
    }
    if (!valid || Math.abs(vals[12] - 1.0) > 0.01) continue;

    let scanPos = i + 110;

    // Read 67_19 entity ID (always present right after 66_19)
    let entityId: string | null = null;
    if (scanPos + 6 <= md.length && md[scanPos] === 0x67 && md[scanPos + 1] === 0x19) {
      const l = md[scanPos + 2] | (md[scanPos + 3] << 8) | (md[scanPos + 4] << 16) | (md[scanPos + 5] << 24);
      if (l > 0 && l <= 8) {
        entityId = hexId(md, scanPos + 6, l);
        scanPos += 6 + l;
      }
    }

    // Skip 68_19 UUID
    if (scanPos + 6 <= md.length && md[scanPos] === 0x68 && md[scanPos + 1] === 0x19) {
      const l = md[scanPos + 2] | (md[scanPos + 3] << 8) | (md[scanPos + 4] << 16) | (md[scanPos + 5] << 24);
      scanPos += 6 + l;
    }

    // Classify by next tag
    let isDefHeader = false;
    let defName: string | null = null;
    let de05Id: string | null = null;

    if (scanPos + 2 <= md.length && md[scanPos] === 0x9e && md[scanPos + 1] === 0x13) {
      // Definition-header instance (template) — get name from 7e_15
      isDefHeader = true;
      for (let j = scanPos; j < Math.min(scanPos + 300, md.length - 10); j++) {
        if (md[j] === 0x7e && md[j + 1] === 0x15) {
          const nlen = md[j + 2] | (md[j + 3] << 8) | (md[j + 4] << 16) | (md[j + 5] << 24);
          if (nlen > 0 && nlen < 300 && j + 6 + nlen <= md.length) {
            defName = new TextDecoder('utf-8')
              .decode(md.subarray(j + 6, j + 6 + nlen))
              .replace(/\0/g, '').trim();
            break;
          }
        }
      }
    } else {
      // Reference instance (actual placement) — extract de_05 from 64_19/4c_1d data
      de05Id = extractDe05FromInstance(md, scanPos);
    }

    rawInstances.push({ offset: i, entityId, transform: vals, isDefHeader, defName, de05Id });
  }

  // Phase 3: Build entity ID → name map from definition-header instances
  const entityIdToName = new Map<string, string>();
  for (const inst of rawInstances) {
    if (inst.isDefHeader && inst.entityId && inst.defName) {
      entityIdToName.set(inst.entityId, inst.defName);
    }
  }

  // Phase 4: Resolve names for all instances
  const instances: InstanceTransform[] = [];
  let resolvedDefHeader = 0, resolvedEntityId = 0, resolvedDe05 = 0, skippedNoName = 0;

  for (const raw of rawInstances) {
    let name = raw.defName;

    // Try entity ID → name (same entity ID as a def-header instance)
    if (!name && raw.entityId) {
      name = entityIdToName.get(raw.entityId) ?? null;
      if (name) resolvedEntityId++;
    }

    // Try de_05 → child ID → definition name
    if (!name && raw.de05Id) {
      name = childIdToDefName.get(raw.de05Id) ?? null;
      if (name) resolvedDe05++;
    }

    if (!name) {
      skippedNoName++;
      continue;
    }

    if (raw.isDefHeader) resolvedDefHeader++;

    instances.push({
      defName: name,
      transform: raw.transform,
      offset: raw.offset,
      isDefinitionHeader: raw.isDefHeader,
    });
  }

  // Log summary
  const placements = instances.filter(i => !i.isDefinitionHeader);
  const templates = instances.filter(i => i.isDefinitionHeader);
  console.log(`SKP: Instance resolution: ${resolvedDefHeader} def-headers, ${resolvedEntityId} via entity ID, ${resolvedDe05} via de_05→child ID, ${skippedNoName} unresolved`);
  console.log(`SKP: Total: ${instances.length} instances (${placements.length} placements + ${templates.length} templates)`);

  const byName = new Map<string, number>();
  for (const inst of placements) {
    byName.set(inst.defName, (byName.get(inst.defName) ?? 0) + 1);
  }
  const sorted = [...byName.entries()].sort((a, b) => b[1] - a[1]);
  for (const [name, count] of sorted.slice(0, 15)) {
    console.log(`  "${name}": ${count} placements`);
  }
  if (sorted.length > 15) console.log(`  ... and ${sorted.length - 15} more`);

  return instances;
}

// ============================================================================
// Transform helpers
// ============================================================================

/**
 * Apply a 66_19 transform to a vertex in SKP space (inches).
 *
 * Transform format: 13 doubles stored as rows of a 3x3 matrix + translation.
 * Using p * M convention (row vector on left):
 *   p'_x = p_x * d[0] + p_y * d[3] + p_z * d[6] + d[9]
 *   p'_y = p_x * d[1] + p_y * d[4] + p_z * d[7] + d[10]
 *   p'_z = p_x * d[2] + p_y * d[5] + p_z * d[8] + d[11]
 */
function applyTransform(v: Vec3, t: number[]): Vec3 {
  return {
    x: v.x * t[0] + v.y * t[3] + v.z * t[6] + t[9],
    y: v.x * t[1] + v.y * t[4] + v.z * t[7] + t[10],
    z: v.x * t[2] + v.y * t[5] + v.z * t[8] + t[11],
  };
}

function isIdentityTransform(t: number[]): boolean {
  return Math.abs(t[0] - 1) < 1e-6 && Math.abs(t[4] - 1) < 1e-6 && Math.abs(t[8] - 1) < 1e-6 &&
         Math.abs(t[1]) < 1e-6 && Math.abs(t[2]) < 1e-6 && Math.abs(t[3]) < 1e-6 &&
         Math.abs(t[5]) < 1e-6 && Math.abs(t[6]) < 1e-6 && Math.abs(t[7]) < 1e-6 &&
         Math.abs(t[9]) < 0.01 && Math.abs(t[10]) < 0.01 && Math.abs(t[11]) < 0.01;
}

/**
 * Compose two transforms: p'' = (p * M_inner + t_inner) * M_outer + t_outer
 * Result: M = M_inner * M_outer, t = t_inner * M_outer + t_outer
 */
function composeTransforms(outer: number[], inner: number[]): number[] {
  const r = new Array(13);
  // Matrix multiplication: M_composed = M_inner * M_outer
  for (let row = 0; row < 3; row++) {
    for (let col = 0; col < 3; col++) {
      r[row * 3 + col] =
        inner[row * 3 + 0] * outer[0 * 3 + col] +
        inner[row * 3 + 1] * outer[1 * 3 + col] +
        inner[row * 3 + 2] * outer[2 * 3 + col];
    }
  }
  // Translation: t_composed = t_inner * M_outer + t_outer
  r[9]  = inner[9] * outer[0] + inner[10] * outer[3] + inner[11] * outer[6] + outer[9];
  r[10] = inner[9] * outer[1] + inner[10] * outer[4] + inner[11] * outer[7] + outer[10];
  r[11] = inner[9] * outer[2] + inner[10] * outer[5] + inner[11] * outer[8] + outer[11];
  r[12] = 1.0;
  return r;
}

/**
 * Find the innermost definition block containing a given file offset.
 */
function findDefinitionForOffset(offset: number, blocks: DefinitionBlock[]): DefinitionBlock | null {
  let best: DefinitionBlock | null = null;
  let bestSize = Infinity;

  for (const block of blocks) {
    if (offset >= block.dataStart && offset < block.dataEnd) {
      const size = block.dataEnd - block.dataStart;
      if (size < bestSize) {
        bestSize = size;
        best = block;
      }
    }
  }

  return best;
}

// ============================================================================
// Scene building with definition/instance transforms
// ============================================================================

type WorldFace = {
  face: SKPFace;
  verts: Vec3[];
  transform: number[] | null;
};

function buildThreeJSScene(
  vertexPositions: Map<number, Vec3>,
  edgeVertices: Map<number, [number, number]>,
  faces: SKPFace[],
  materialEntityMap: Map<number, string>,
  materialColors: Map<string, SKPMaterialInfo>,
  zipFiles: Record<string, Uint8Array>,
  definitionBlocks: DefinitionBlock[],
  instanceTransforms: InstanceTransform[],
  onProgress?: ProgressCallback,
): ThreeJSResult {
  // Step 1: Resolve all face vertices (edge chain → ordered vertex loop)
  const resolvedFaces: { face: SKPFace; verts: Vec3[] }[] = [];

  for (const face of faces) {
    const vertIds = resolveFaceVertices(face, edgeVertices);
    if (vertIds.length < 3) continue;

    const verts: Vec3[] = [];
    let allFound = true;
    for (const vid of vertIds) {
      const pos = vertexPositions.get(vid);
      if (!pos) { allFound = false; break; }
      verts.push(pos);
    }
    if (!allFound || verts.length < 3) continue;

    resolvedFaces.push({ face, verts });
  }

  console.log(`SKP: ${resolvedFaces.length}/${faces.length} faces resolved`);

  // Step 2: Classify faces by definition block
  const rootFaces: typeof resolvedFaces = [];
  const defFacesMap = new Map<string, typeof resolvedFaces>();

  for (const rf of resolvedFaces) {
    const block = findDefinitionForOffset(rf.face.fileOffset, definitionBlocks);
    if (!block || block.name === '__root__') {
      rootFaces.push(rf);
    } else {
      if (!defFacesMap.has(block.name)) defFacesMap.set(block.name, []);
      defFacesMap.get(block.name)!.push(rf);
    }
  }

  console.log(`SKP: ${rootFaces.length} root faces, ${defFacesMap.size} definitions with geometry`);

  // Build lookup: definition name → instance transforms
  const instancesByDef = new Map<string, InstanceTransform[]>();
  for (const inst of instanceTransforms) {
    if (!instancesByDef.has(inst.defName)) instancesByDef.set(inst.defName, []);
    instancesByDef.get(inst.defName)!.push(inst);
  }

  // Detect nested instances: instances whose offset falls within a parent definition's range
  // Build definition section ranges using 7e_15 name offsets for broader, contiguous sections.
  // A definition's section spans from its 7e_15 name tag to the next definition's name tag.
  // This ensures 66_19 instance tags (which appear between name and geometry, or after geometry)
  // are correctly attributed to their parent definition.
  const defNameStarts = new Map<string, number>();
  for (const block of definitionBlocks) {
    if (block.name === '__unassigned__') continue;
    const existing = defNameStarts.get(block.name);
    if (!existing || block.nameOffset < existing) {
      defNameStarts.set(block.name, block.nameOffset);
    }
  }

  // Sort definitions by their name offset to compute contiguous section boundaries
  const sortedDefEntries = [...defNameStarts.entries()].sort((a, b) => a[1] - b[1]);
  const maxDataEnd = definitionBlocks.reduce((mx, b) => Math.max(mx, b.dataEnd), 0);

  const defSections = new Map<string, { start: number; end: number }>();
  for (let i = 0; i < sortedDefEntries.length; i++) {
    const [name, start] = sortedDefEntries[i];
    const end = i + 1 < sortedDefEntries.length ? sortedDefEntries[i + 1][1] : maxDataEnd;
    defSections.set(name, { start, end });
  }

  console.log(`SKP: ${defSections.size} definition sections for nesting detection`);
  // Log top-5 largest sections
  const sortedSections = [...defSections.entries()].sort((a, b) => (b[1].end - b[1].start) - (a[1].end - a[1].start));
  for (const [name, range] of sortedSections.slice(0, 5)) {
    console.log(`  Section "${name}": offset ${range.start}–${range.end} (${((range.end - range.start) / 1024).toFixed(0)} KB)`);
  }

  // Classify each instance as root-level or nested (inside another definition)
  let nestedCount = 0;
  for (const inst of instanceTransforms) {
    let bestParent: string | null = null;
    let bestSize = Infinity;
    for (const [secName, range] of defSections) {
      if (secName === inst.defName) continue;
      if (inst.offset >= range.start && inst.offset <= range.end) {
        const size = range.end - range.start;
        if (size < bestSize) {
          bestSize = size;
          bestParent = secName;
        }
      }
    }
    inst.parentDef = bestParent;
    if (bestParent) nestedCount++;
  }

  if (nestedCount > 0) {
    console.log(`SKP: ${nestedCount}/${instanceTransforms.length} instances are nested inside parent definitions`);
    const byParent = new Map<string, string[]>();
    for (const inst of instanceTransforms) {
      if (inst.parentDef) {
        if (!byParent.has(inst.parentDef)) byParent.set(inst.parentDef, []);
        const list = byParent.get(inst.parentDef)!;
        if (!list.includes(inst.defName)) list.push(inst.defName);
      }
    }
    for (const [parent, children] of byParent) {
      console.log(`  "${parent}" contains: ${children.join(', ')}`);
    }
  } else {
    console.log(`SKP: No nested instances detected — all ${instanceTransforms.length} instances are root-level`);
  }

  // Log per-definition instance summary (root vs nested)
  for (const [defName, instances] of instancesByDef) {
    const rootCount = instances.filter(i => !i.parentDef).length;
    const nestedInst = instances.filter(i => i.parentDef);
    if (nestedInst.length > 0) {
      const parents = [...new Set(nestedInst.map(i => i.parentDef))];
      console.log(`SKP: "${defName}": ${rootCount} root + ${nestedInst.length} nested (in: ${parents.join(', ')})`);
    }
  }

  // Step 3: Spatial filtering — determine which identity definitions to render
  // Some definitions (e.g. 3D Warehouse components) have geometry in local coordinates
  // far from the model origin. Without matching instance transforms, rendering them at
  // identity produces "weird objects" floating in space. We filter these out by checking
  // if their geometry overlaps with a reference bounding box.

  // 3a: Identify identity definitions (no matching instances) sorted by face count
  const identityDefEntries: [string, typeof resolvedFaces][] = [];
  const instancedDefEntries: [string, typeof resolvedFaces][] = [];
  for (const [defName, defFaces] of defFacesMap) {
    const instances = instancesByDef.get(defName);
    // Only count placement instances (not definition-header templates)
    const placementInstances = instances?.filter(i => !i.isDefinitionHeader);
    if (placementInstances && placementInstances.length > 0) {
      instancedDefEntries.push([defName, defFaces]);
    } else {
      identityDefEntries.push([defName, defFaces]);
    }
  }
  identityDefEntries.sort((a, b) => b[1].length - a[1].length);

  // 3b: Compute reference bounding box from root faces + largest identity definition
  // The largest identity definition is almost always the main building/terrain.
  function computeVertBBox(faceSets: typeof resolvedFaces[]) {
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

  const refSets: typeof resolvedFaces[] = [rootFaces];
  if (identityDefEntries.length > 0) refSets.push(identityDefEntries[0][1]);
  const refBBox = computeVertBBox(refSets);

  // Expand reference bbox by 25% of its size (was 100% — too permissive)
  const refDiagX = refBBox.maxX - refBBox.minX;
  const refDiagY = refBBox.maxY - refBBox.minY;
  const refDiagZ = refBBox.maxZ - refBBox.minZ;
  const marginX = Math.max(refDiagX * 0.25, 200); // ~5m in inches
  const marginY = Math.max(refDiagY * 0.25, 200);
  const marginZ = Math.max(refDiagZ * 0.25, 200);
  const hasInstancedDefs = instancedDefEntries.length > 0;

  function defCenterInsideRef(defFaces: typeof resolvedFaces, defName: string): boolean {
    // Compute center + bbox of definition's geometry
    let cx = 0, cy = 0, cz = 0, n = 0;
    let dMinX = Infinity, dMinY = Infinity, dMinZ = Infinity;
    let dMaxX = -Infinity, dMaxY = -Infinity, dMaxZ = -Infinity;
    for (const rf of defFaces) {
      for (const v of rf.verts) {
        cx += v.x; cy += v.y; cz += v.z; n++;
        if (v.x < dMinX) dMinX = v.x;
        if (v.y < dMinY) dMinY = v.y;
        if (v.z < dMinZ) dMinZ = v.z;
        if (v.x > dMaxX) dMaxX = v.x;
        if (v.y > dMaxY) dMaxY = v.y;
        if (v.z > dMaxZ) dMaxZ = v.z;
      }
    }
    if (n === 0) return false;
    cx /= n; cy /= n; cz /= n;

    // If the model uses instances and this definition is small compared to reference,
    // it's likely a component that should have had an instance transform → skip it
    const defSizeX = dMaxX - dMinX;
    const defSizeY = dMaxY - dMinY;
    const defSizeZ = dMaxZ - dMinZ;
    if (hasInstancedDefs && defFaces.length < 500) {
      const defMaxDim = Math.max(defSizeX, defSizeY, defSizeZ);
      const refMaxDim = Math.max(refDiagX, refDiagY, refDiagZ);
      if (defMaxDim < refMaxDim * 0.15) {
        console.log(`SKP: Skipping small identity def "${defName}" (${defFaces.length} faces, size ${(defMaxDim * 0.0254).toFixed(2)}m, likely missing instance)`);
        return false;
      }
    }

    return cx >= refBBox.minX - marginX && cx <= refBBox.maxX + marginX &&
           cy >= refBBox.minY - marginY && cy <= refBBox.maxY + marginY &&
           cz >= refBBox.minZ - marginZ && cz <= refBBox.maxZ + marginZ;
  }

  // Step 4: Build world-space faces
  // Root faces: render at identity
  // Definition faces: render per instance with transform, or at identity if spatially valid
  const worldFacesByGroup = new Map<string, WorldFace[]>();

  // Root faces → group "Model"
  if (rootFaces.length > 0) {
    worldFacesByGroup.set('Model', rootFaces.map(rf => ({
      face: rf.face,
      verts: rf.verts,
      transform: null,
    })));
  }

  let identityDefs = 0;
  let skippedOutliers = 0;
  let instancedDefs = 0;
  let instancedFaces = 0;

  // Identity definitions: render at identity only if geometry is inside reference bbox
  for (const [defName, defFaces] of identityDefEntries) {
    // First/largest definition always included (it IS the reference)
    const isReference = identityDefEntries[0][0] === defName;
    if (isReference || defCenterInsideRef(defFaces, defName)) {
      identityDefs++;
      worldFacesByGroup.set(defName, defFaces.map(rf => ({
        face: rf.face,
        verts: rf.verts,
        transform: null,
      })));
    } else {
      skippedOutliers++;
      console.log(`SKP: Skipping outlier definition "${defName}" (${defFaces.length} faces, outside reference bbox)`);
    }
  }

  // Instanced definitions: render through instance transforms (placements only, not templates)
  // Handle nested instances by composing parent + child transforms
  for (const [defName, defFaces] of instancedDefEntries) {
    const instances = instancesByDef.get(defName)!.filter(i => !i.isDefinitionHeader);
    instancedDefs++;
    const groupFaces: WorldFace[] = [];

    for (const inst of instances) {
      if (!inst.parentDef) {
        // Root-level instance: use transform directly
        for (const rf of defFaces) {
          groupFaces.push({
            face: rf.face,
            verts: rf.verts,
            transform: isIdentityTransform(inst.transform) ? null : inst.transform,
          });
        }
      } else {
        // Nested instance: compose with parent's root-level transforms
        const parentInstances = instancesByDef.get(inst.parentDef);
        if (parentInstances) {
          // Find root-level instances of the parent (non-nested)
          const parentRoots = parentInstances.filter(pi => !pi.parentDef);
          if (parentRoots.length > 0) {
            for (const parentInst of parentRoots) {
              const composed = composeTransforms(parentInst.transform, inst.transform);
              for (const rf of defFaces) {
                groupFaces.push({
                  face: rf.face,
                  verts: rf.verts,
                  transform: isIdentityTransform(composed) ? null : composed,
                });
              }
            }
          } else {
            // Parent has no root instances — try composing with parent's nested instances
            // (handles multi-level nesting, one extra level)
            for (const parentInst of parentInstances) {
              if (parentInst.parentDef) {
                const grandParentInstances = instancesByDef.get(parentInst.parentDef);
                if (grandParentInstances) {
                  for (const gpInst of grandParentInstances.filter(g => !g.parentDef)) {
                    const composed = composeTransforms(
                      composeTransforms(gpInst.transform, parentInst.transform),
                      inst.transform,
                    );
                    for (const rf of defFaces) {
                      groupFaces.push({
                        face: rf.face,
                        verts: rf.verts,
                        transform: isIdentityTransform(composed) ? null : composed,
                      });
                    }
                  }
                }
              }
            }
          }
        } else {
          // Parent definition has no instances (it's an identity def) — use transform as-is
          // The parent is rendered at identity, so this transform is already in parent space
          for (const rf of defFaces) {
            groupFaces.push({
              face: rf.face,
              verts: rf.verts,
              transform: isIdentityTransform(inst.transform) ? null : inst.transform,
            });
          }
        }
      }
    }

    instancedFaces += groupFaces.length;
    worldFacesByGroup.set(defName, groupFaces);
  }

  console.log(`SKP: ${identityDefs} definitions at identity, ${skippedOutliers} outliers skipped, ${instancedDefs} instanced (${instancedFaces} faces)`);

  // Step 4: Build Three.js scene — one Group per definition, meshes per material inside
  const root = new THREE.Group();
  root.name = 'SKP Scene';
  let totalVerts = 0;
  let totalTris = 0;
  let groupIdx = 0;
  const groupNames = [...worldFacesByGroup.keys()];

  for (const groupName of groupNames) {
    groupIdx++;
    onProgress?.(0.65 + (groupIdx / groupNames.length) * 0.3,
      `Groep ${groupIdx}/${groupNames.length}: ${groupName}`);

    const groupFaces = worldFacesByGroup.get(groupName)!;

    // Sub-group by material
    const byMaterial = new Map<string, WorldFace[]>();
    const noMaterial: WorldFace[] = [];

    for (const wf of groupFaces) {
      const matRef = wf.face.materialRef;
      if (matRef !== null && materialEntityMap.has(matRef)) {
        const matName = materialEntityMap.get(matRef)!;
        if (!byMaterial.has(matName)) byMaterial.set(matName, []);
        byMaterial.get(matName)!.push(wf);
      } else {
        noMaterial.push(wf);
      }
    }

    const defGroup = new THREE.Group();
    defGroup.name = groupName;
    let groupTris = 0;

    // Build mesh per material
    for (const [matName, matFaces] of byMaterial) {
      const { geometry, vertCount, triCount } = buildFaceGeometryWithTransform(matFaces);
      if (triCount === 0) continue;

      const matInfo = materialColors.get(matName);
      const material = new THREE.MeshStandardMaterial({
        color: matInfo
          ? new THREE.Color(matInfo.colorR / 255, matInfo.colorG / 255, matInfo.colorB / 255)
          : 0xcccccc,
        side: THREE.DoubleSide,
        roughness: 0.7,
        metalness: 0.1,
        opacity: matInfo?.opacity ?? 1,
        transparent: (matInfo?.opacity ?? 1) < 0.99,
      });

      if (matInfo?.textureFile && zipFiles[matInfo.textureFile]) {
        loadTextureForMaterial(material, zipFiles[matInfo.textureFile], matInfo.textureFile);
      }

      const mesh = new THREE.Mesh(geometry, material);
      mesh.name = matName;
      defGroup.add(mesh);
      totalVerts += vertCount;
      totalTris += triCount;
      groupTris += triCount;
    }

    // Default mesh (no material)
    if (noMaterial.length > 0) {
      const { geometry, vertCount, triCount } = buildFaceGeometryWithTransform(noMaterial);
      if (triCount > 0) {
        const material = new THREE.MeshStandardMaterial({
          color: 0xcccccc,
          side: THREE.DoubleSide,
          roughness: 0.7,
          metalness: 0.1,
        });
        const mesh = new THREE.Mesh(geometry, material);
        mesh.name = 'Default';
        defGroup.add(mesh);
        totalVerts += vertCount;
        totalTris += triCount;
        groupTris += triCount;
      }
    }

    if (defGroup.children.length > 0) {
      root.add(defGroup);
      console.log(`  Group "${groupName}": ${groupFaces.length} faces, ${groupTris} tris, ${defGroup.children.length} meshes`);
    }
  }

  console.log(`SKP: ${totalTris} triangles, ${totalVerts} vertices, ${root.children.length} groups`);

  // Compute bounding box
  const bb = new THREE.Box3();
  root.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      child.geometry.computeBoundingBox();
      if (child.geometry.boundingBox) {
        bb.expandByObject(child);
      }
    }
  });

  if (bb.isEmpty()) {
    bb.set(new THREE.Vector3(-1, -1, -1), new THREE.Vector3(1, 1, 1));
  }

  return {
    scene: root,
    totalVertices: totalVerts,
    totalTriangles: totalTris,
    bounds: { min: bb.min.clone(), max: bb.max.clone() },
  };
}

// ============================================================================
// Face geometry builder with transform support
// ============================================================================

/**
 * Project a 3D polygon onto a 2D plane for ear-clipping triangulation.
 * Uses the face normal to determine the best projection plane.
 */
function projectTo2D(verts: Vec3[], nx: number, ny: number, nz: number): number[] {
  const ax = Math.abs(nx), ay = Math.abs(ny), az = Math.abs(nz);
  const coords: number[] = [];

  if (ax >= ay && ax >= az) {
    // Drop X, project onto YZ
    for (const v of verts) { coords.push(v.y, v.z); }
  } else if (ay >= ax && ay >= az) {
    // Drop Y, project onto XZ
    for (const v of verts) { coords.push(v.x, v.z); }
  } else {
    // Drop Z, project onto XY
    for (const v of verts) { coords.push(v.x, v.y); }
  }
  return coords;
}

function buildFaceGeometryWithTransform(
  worldFaces: WorldFace[],
): { geometry: THREE.BufferGeometry; vertCount: number; triCount: number } {
  const positions: number[] = [];
  const normals: number[] = [];

  for (const { face, verts, transform } of worldFaces) {
    // Apply instance transform to vertices (in SKP space, inches)
    const tv: Vec3[] = transform
      ? verts.map(v => applyTransform(v, transform))
      : verts;

    if (tv.length < 3) continue;

    // Calculate normal from (transformed) vertices
    let nx: number, ny: number, nz: number;
    if (!transform && face.plane) {
      nx = face.plane.nx;
      ny = face.plane.ny;
      nz = face.plane.nz;
    } else {
      // Newell's method for robust normal computation
      nx = 0; ny = 0; nz = 0;
      for (let i = 0; i < tv.length; i++) {
        const cur = tv[i];
        const next = tv[(i + 1) % tv.length];
        nx += (cur.y - next.y) * (cur.z + next.z);
        ny += (cur.z - next.z) * (cur.x + next.x);
        nz += (cur.x - next.x) * (cur.y + next.y);
      }
      const len = Math.sqrt(nx * nx + ny * ny + nz * nz);
      if (len > 1e-12) { nx /= len; ny /= len; nz /= len; }
      else { nx = 0; ny = 0; nz = 1; }
    }

    // Triangulate: use ear-clipping for 5+ vertices, simple fan for 3-4
    let triIndices: number[];
    if (tv.length <= 4) {
      // Simple fan is safe for triangles and most quads
      triIndices = [];
      for (let t = 1; t < tv.length - 1; t++) {
        triIndices.push(0, t, t + 1);
      }
    } else {
      // Ear-clipping triangulation via 2D projection
      const coords2D = projectTo2D(tv, nx, ny, nz);
      const earIndices = Earcut.triangulate(coords2D, undefined, 2);
      if (earIndices.length === 0) {
        // Fallback to fan if earcut fails
        triIndices = [];
        for (let t = 1; t < tv.length - 1; t++) {
          triIndices.push(0, t, t + 1);
        }
      } else {
        triIndices = earIndices;
      }
    }

    // Emit triangles
    for (let t = 0; t < triIndices.length; t += 3) {
      const v0 = tv[triIndices[t]];
      const v1 = tv[triIndices[t + 1]];
      const v2 = tv[triIndices[t + 2]];
      if (!v0 || !v1 || !v2) continue;

      // Convert: inches → meters, SketchUp (X,Y,Z) → Three.js (X,Z,-Y)
      positions.push(
        v0.x * INCHES_TO_METERS, v0.z * INCHES_TO_METERS, -v0.y * INCHES_TO_METERS,
        v1.x * INCHES_TO_METERS, v1.z * INCHES_TO_METERS, -v1.y * INCHES_TO_METERS,
        v2.x * INCHES_TO_METERS, v2.z * INCHES_TO_METERS, -v2.y * INCHES_TO_METERS,
      );
      normals.push(
        nx, nz, -ny,
        nx, nz, -ny,
        nx, nz, -ny,
      );
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(positions), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(normals), 3));

  return {
    geometry,
    vertCount: positions.length / 3,
    triCount: positions.length / 9,
  };
}

// ============================================================================
// Vertex loop resolution
// ============================================================================

function resolveFaceVertices(
  face: SKPFace,
  edgeVertices: Map<number, [number, number]>,
): number[] {
  if (face.edgeRefs.length === 0) return [];

  const edgePairs: [number, number][] = [];
  for (const ref of face.edgeRefs) {
    const pair = edgeVertices.get(ref.id);
    if (!pair) continue;
    if (ref.reversed) {
      edgePairs.push([pair[1], pair[0]]);
    } else {
      edgePairs.push([pair[0], pair[1]]);
    }
  }

  if (edgePairs.length === 0) return [];

  const ordered: number[] = [edgePairs[0][0], edgePairs[0][1]];
  const used = new Set<number>([0]);

  for (let step = 0; step < edgePairs.length - 1; step++) {
    const lastVert = ordered[ordered.length - 1];
    let found = false;

    for (let i = 0; i < edgePairs.length; i++) {
      if (used.has(i)) continue;
      const [a, b] = edgePairs[i];
      if (a === lastVert) {
        ordered.push(b);
        used.add(i);
        found = true;
        break;
      } else if (b === lastVert) {
        ordered.push(a);
        used.add(i);
        found = true;
        break;
      }
    }

    if (!found) break;
  }

  if (ordered.length > 1 && ordered[ordered.length - 1] === ordered[0]) {
    ordered.pop();
  }

  return ordered;
}

// ============================================================================
// Texture loading
// ============================================================================

function loadTextureForMaterial(
  material: THREE.MeshStandardMaterial,
  textureData: Uint8Array,
  texturePath: string,
): void {
  try {
    const ext = texturePath.split('.').pop()?.toLowerCase();
    const mimeType = ext === 'png' ? 'image/png' : 'image/jpeg';
    const blob = new Blob([new Uint8Array(textureData)], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const loader = new THREE.TextureLoader();
    loader.load(url, (texture) => {
      texture.wrapS = THREE.RepeatWrapping;
      texture.wrapT = THREE.RepeatWrapping;
      material.map = texture;
      material.needsUpdate = true;
      URL.revokeObjectURL(url);
    });
  } catch {
    // Skip failed textures
  }
}
