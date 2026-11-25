// Run with Node 24 against web commit b9b0a2736642b37dd45925a4719dfef08b8f36b4.
const fs = require('node:fs');
const path = require('node:path');
const { stripTypeScriptTypes } = require('node:module');
if (!process.argv[2]) throw new Error('Usage: node tools/generate_soldier_drop_reference.cjs /path/to/defendtaipei-web');
const source = fs.readFileSync(path.join(process.argv[2], 'src/systems/pla-loot.ts'), 'utf8');
const math = Object.create(Math);
const drop = new Function('Math', stripTypeScriptTypes(source).replace(/\bexport /g, '') + ';return plaDrop;')(math);
const cases = [];
for (let id = 0; id < 64; id++) {
  let seed = id + 1;
  math.random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
  cases.push({ id, items: Object.entries(drop(id)), final_seed: seed });
}
fs.writeFileSync(path.resolve(__dirname, '../game/tests/fixtures/soldier-drop-reference.json'), JSON.stringify(cases, null, 2) + '\n');
console.log('64 soldier drops from original TypeScript');
