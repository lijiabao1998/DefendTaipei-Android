// Run with Node 24 against web commit b9b0a2736642b37dd45925a4719dfef08b8f36b4.
const fs = require('node:fs');
const path = require('node:path');
const { stripTypeScriptTypes } = require('node:module');
const webRoot = process.argv[2];
if (!webRoot) throw new Error('Usage: node tools/generate_world_loot_reference.cjs /path/to/defendtaipei-web');
const itemsSource = fs.readFileSync(path.join(webRoot, 'src/systems/items.ts'), 'utf8');
const itemsJs = stripTypeScriptTypes(itemsSource.replace(/^import[^\n]+\n/, '')).replace(/\bexport /g, '');
const { ITEMS, LOOT } = new Function('tr', itemsJs + ';return {ITEMS, LOOT};')((zh) => zh);
const main = fs.readFileSync(path.join(webRoot, 'src/core/main.ts'), 'utf8');
const from = main.indexOf('function rollLoot(');
const until = main.indexOf('const dTile =', from);
if (from < 0 || until < 0) throw new Error('Original rollLoot/spoilRolled source block not found');
const source = stripTypeScriptTypes(main.slice(from, until));
const cases = [];
for (const minute of [440, 5459, 5460]) {
  const roll = new Function('LOOT', 'ITEMS', 'gameMin', 'POWER_OFF', source + ';return rollLoot;')(LOOT, ITEMS, minute, 4020);
  for (const kind of [...Object.keys(LOOT), 'unknown']) {
    for (const initial_seed of [0, 1, 29, 0x12345678, 20261024, 0x80000000, 0xfffffff0, 0xffffffff]) {
      let seed = initial_seed;
      const random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
      const items = Object.entries(roll(kind, random));
      cases.push({ kind, minute, initial_seed, items, final_seed: seed });
    }
  }
}
const target = path.resolve(__dirname, '../game/tests/fixtures/world-loot-reference.json');
fs.writeFileSync(target, JSON.stringify({
  source: 'lijiabao1998/defendtaipei@b9b0a2736642b37dd45925a4719dfef08b8f36b4',
  file: 'src/core/main.ts',
  note: 'Executed unmodified rollLoot and spoilRolled with original item tables; includes the exact spoilage boundary and unknown-table fallback. Native container placement is a separate stream.',
  cases,
}, null, 2) + '\n');
console.log(cases.length + ' loot cases from original TypeScript');
