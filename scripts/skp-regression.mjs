// SketchUp import regression check: parses every .skp in test-models/ (and public/) with openskp,
// the same way SKPLoader.ts does, and compares with test-models/skp-expected.json.
//   node scripts/skp-regression.mjs            compare against the baseline
//   node scripts/skp-regression.mjs --update   write a new baseline
import fs from 'node:fs';
import path from 'node:path';
import { buildInstancedScene, toInstancedGLB } from 'openskp';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..');
const dirs = ['test-models', 'public'];
const baselinePath = path.join(root, 'test-models', 'skp-expected.json');
const update = process.argv.includes('--update');

const results = {};
for (const dir of dirs) {
  for (const f of fs.readdirSync(path.join(root, dir)).filter((n) => n.toLowerCase().endsWith('.skp')).sort()) {
    const buf = fs.readFileSync(path.join(root, dir, f));
    const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
    const t = Date.now();
    try {
      const scene = buildInstancedScene(ab);
      const triangles = scene.meshResources.reduce((s, r) => s + r.primitives.reduce((p, q) => p + q.indices.length / 3, 0), 0);
      const glb = toInstancedGLB(scene, { textures: true });
      const b = scene.bounds;
      results[`${dir}/${f}`] = {
        ok: true,
        meshes: scene.meshResources.length,
        triangles,
        size: b ? b.size.map((v) => Math.round(v * 1000) / 1000) : null,
        glbBytes: glb.byteLength,
      };
      console.log(`OK   ${(dir + '/' + f).padEnd(48)} meshes ${String(scene.meshResources.length).padStart(5)} tris ${String(triangles).padStart(8)} size ${JSON.stringify(results[`${dir}/${f}`].size)} ${Date.now() - t} ms`);
    } catch (e) {
      results[`${dir}/${f}`] = { ok: false, error: String(e?.message ?? e).slice(0, 160) };
      console.log(`FAIL ${(dir + '/' + f).padEnd(48)} ${results[`${dir}/${f}`].error}`);
    }
  }
}

if (update || !fs.existsSync(baselinePath)) {
  fs.writeFileSync(baselinePath, JSON.stringify(results, null, 2) + '\n');
  console.log(`baseline written: ${baselinePath}`);
  process.exit(0);
}
const base = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
let diffs = 0;
for (const [k, v] of Object.entries(results)) {
  const b = base[k];
  const same = b && b.ok === v.ok && b.meshes === v.meshes && b.triangles === v.triangles && JSON.stringify(b.size) === JSON.stringify(v.size);
  if (!same) {
    diffs++;
    console.log(`DIFF ${k}: expected ${JSON.stringify(b)} got ${JSON.stringify(v)}`);
  }
}
console.log(diffs ? `${diffs} difference(s)` : 'all files match the baseline');
process.exit(diffs ? 1 : 0);
