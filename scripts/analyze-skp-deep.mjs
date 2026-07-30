/**
 * Deep analysis of unnamed 66_19 instances — what tags appear between them?
 * This reveals how SketchUp links instances to definitions when 7e_15 is absent.
 */

import { readFileSync } from 'fs';
import { inflateRawSync, inflateSync } from 'zlib';

const filePath = process.argv[2];
if (!filePath) { console.error('Usage: node analyze-skp-deep.mjs <file>'); process.exit(1); }

const raw = readFileSync(filePath);

// Find ZIP
let zipStart = -1;
for (let i = 0; i < Math.min(raw.length, 500); i++) {
  if (raw[i] === 0x50 && raw[i+1] === 0x4b && raw[i+2] === 0x03 && raw[i+3] === 0x04) { zipStart = i; break; }
}

// Parse ZIP entries
const entries = [];
let pos = zipStart;
while (pos < raw.length - 30) {
  if (raw[pos] !== 0x50 || raw[pos+1] !== 0x4b || raw[pos+2] !== 0x03 || raw[pos+3] !== 0x04) break;
  const comprMethod = raw[pos+8] | (raw[pos+9] << 8);
  const compSize = raw[pos+18] | (raw[pos+19] << 8) | (raw[pos+20] << 16) | (raw[pos+21] << 24);
  const nameLen = raw[pos+26] | (raw[pos+27] << 8);
  const extraLen = raw[pos+28] | (raw[pos+29] << 8);
  const name = raw.slice(pos+30, pos+30+nameLen).toString('utf-8');
  const dataStart = pos + 30 + nameLen + extraLen;
  entries.push({ name, comprMethod, compSize, dataStart });
  pos = dataStart + compSize;
}

const modelEntry = entries.find(e => e.name === 'model.dat') || entries.reduce((b, e) => (!b || e.compSize > b.compSize) ? e : b, null);
let md;
try { md = inflateSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize)); }
catch { md = inflateRawSync(raw.slice(modelEntry.dataStart, modelEntry.dataStart + modelEntry.compSize)); }

const dv = new DataView(md.buffer, md.byteOffset, md.byteLength);

// Find ALL tags (2-byte tag + 4-byte length) in a region
function scanTags(start, end) {
  const tags = [];
  let i = start;
  while (i < end - 6) {
    const tag = (md[i] << 8) | md[i+1];
    const tagHex = tag.toString(16).padStart(4, '0');
    const hi = md[i].toString(16).padStart(2, '0');
    const lo = md[i+1].toString(16).padStart(2, '0');
    const len = md[i+2] | (md[i+3] << 8) | (md[i+4] << 16) | (md[i+5] << 24);

    // Sanity check: length must be reasonable
    if (len >= 0 && len < 1000000 && i + 6 + len <= end + 100000) {
      tags.push({ offset: i, tag: `${hi}_${lo}`, len, dataStart: i + 6 });
      i = i + 6 + len;
    } else {
      i++;
    }
  }
  return tags;
}

// Find all 66_19 transforms
const allInstances = [];
for (let i = 0; i < md.length - 110; i++) {
  if (md[i] === 0x66 && md[i+1] === 0x19) {
    const len = md[i+2] | (md[i+3] << 8) | (md[i+4] << 16) | (md[i+5] << 24);
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
    allInstances.push({ offset: i, endOffset: i + 110, vals });
  }
}

console.log(`Total 66_19 instances: ${allInstances.length}\n`);

// Analyze the bytes BETWEEN consecutive 66_19 instances
// and BEFORE the first 66_19 in clusters
console.log('=== ANALYZING BYTES BETWEEN 66_19 INSTANCES ===\n');

// Look at the first cluster of unnamed instances (around offset 3649089)
const clusters = [];
let currentCluster = [allInstances[0]];
for (let i = 1; i < allInstances.length; i++) {
  const gap = allInstances[i].offset - allInstances[i-1].endOffset;
  if (gap < 500) {
    currentCluster.push(allInstances[i]);
  } else {
    if (currentCluster.length > 1) clusters.push(currentCluster);
    currentCluster = [allInstances[i]];
  }
}
if (currentCluster.length > 1) clusters.push(currentCluster);

console.log(`Found ${clusters.length} clusters of closely-packed instances:\n`);
for (let ci = 0; ci < Math.min(clusters.length, 10); ci++) {
  const cluster = clusters[ci];
  console.log(`--- Cluster ${ci+1}: ${cluster.length} instances, starting @${cluster[0].offset} ---`);

  // Look at tags BEFORE the cluster (200 bytes before)
  const preStart = Math.max(0, cluster[0].offset - 300);
  const preTags = scanTags(preStart, cluster[0].offset);
  console.log(`  Tags before cluster (${preStart}–${cluster[0].offset}):`);
  for (const t of preTags.slice(-5)) {
    let extra = '';
    if (t.tag === '7e_15') {
      const nameBytes = md.slice(t.dataStart, t.dataStart + Math.min(t.len, 100));
      extra = ` name="${Buffer.from(nameBytes).toString('utf-8').replace(/\0/g,'')}"`;
    }
    console.log(`    @${t.offset}: ${t.tag} len=${t.len}${extra}`);
  }

  // Look at tags between each pair of instances in the cluster
  for (let j = 0; j < Math.min(cluster.length, 4); j++) {
    const inst = cluster[j];
    const tx = (inst.vals[9] * 0.0254).toFixed(3);
    const ty = (inst.vals[10] * 0.0254).toFixed(3);
    const tz = (inst.vals[11] * 0.0254).toFixed(3);
    console.log(`  Instance ${j}: @${inst.offset} translate=(${tx}, ${ty}, ${tz})m`);

    // Tags AFTER this instance until next instance (or +500 bytes)
    const nextOffset = j + 1 < cluster.length ? cluster[j+1].offset : inst.endOffset + 500;
    const betweenTags = scanTags(inst.endOffset, nextOffset);
    for (const t of betweenTags) {
      let extra = '';
      if (t.tag === '7e_15') {
        const nameBytes = md.slice(t.dataStart, t.dataStart + Math.min(t.len, 100));
        extra = ` name="${Buffer.from(nameBytes).toString('utf-8').replace(/\0/g,'')}"`;
      }
      // Show raw bytes for small data
      if (t.len <= 16) {
        const dataBytes = Array.from(md.slice(t.dataStart, t.dataStart + t.len))
          .map(b => b.toString(16).padStart(2, '0')).join(' ');
        extra += ` data=[${dataBytes}]`;
      }
      console.log(`    @${t.offset}: ${t.tag} len=${t.len}${extra}`);
    }
  }
  console.log();
}

// Also look at a NAMED instance for comparison
console.log('\n=== NAMED INSTANCE COMPARISON ===\n');
// Find instances where a 7e_15 IS nearby
for (const inst of allInstances.slice(0, 100)) {
  // Look forward for 7e_15
  for (let j = inst.endOffset; j < Math.min(inst.endOffset + 500, md.length - 10); j++) {
    if (md[j] === 0x7e && md[j+1] === 0x15) {
      const nlen = md[j+2] | (md[j+3] << 8) | (md[j+4] << 16) | (md[j+5] << 24);
      if (nlen > 0 && nlen < 300) {
        const name = Buffer.from(md.slice(j+6, j+6+nlen)).toString('utf-8').replace(/\0/g,'').trim();
        console.log(`Named instance @${inst.offset} → "${name}" (7e_15 at offset ${j}, gap=${j - inst.endOffset} bytes)`);

        // Show ALL tags between 66_19 end and 7e_15
        const tags = scanTags(inst.endOffset, j + 6 + nlen + 100);
        for (const t of tags) {
          let extra = '';
          if (t.tag === '7e_15') {
            const nameBytes = md.slice(t.dataStart, t.dataStart + Math.min(t.len, 100));
            extra = ` name="${Buffer.from(nameBytes).toString('utf-8').replace(/\0/g,'')}"`;
          }
          if (t.len <= 16) {
            const dataBytes = Array.from(md.slice(t.dataStart, t.dataStart + t.len))
              .map(b => b.toString(16).padStart(2, '0')).join(' ');
            extra += ` data=[${dataBytes}]`;
          }
          console.log(`  @${t.offset}: ${t.tag} len=${t.len}${extra}`);
        }
        console.log();
        break;
      }
    }
    // Stop at another 66_19
    if (md[j] === 0x66 && md[j+1] === 0x19) {
      const nl = md[j+2] | (md[j+3] << 8) | (md[j+4] << 16) | (md[j+5] << 24);
      if (nl === 104) break;
    }
  }
}

// Analyze the specific tags that appear between unnamed instances
console.log('\n=== TAG FREQUENCY BETWEEN UNNAMED INSTANCES ===\n');
const tagFreq = new Map();
for (const cluster of clusters) {
  for (let j = 0; j < cluster.length; j++) {
    const inst = cluster[j];
    const nextOffset = j + 1 < cluster.length ? cluster[j+1].offset : inst.endOffset + 500;
    const tags = scanTags(inst.endOffset, nextOffset);
    for (const t of tags) {
      tagFreq.set(t.tag, (tagFreq.get(t.tag) || 0) + 1);
    }
  }
}
const sortedFreq = [...tagFreq.entries()].sort((a, b) => b[1] - a[1]);
for (const [tag, count] of sortedFreq) {
  console.log(`  ${tag}: ${count} occurrences`);
}
