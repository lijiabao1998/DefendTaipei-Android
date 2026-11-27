// Execute the pinned original gameplay functions; no renderer or native reimplementation.
// Node 24: node tools/generate_armor_reference.cjs /path/to/defendtaipei-web
const fs = require('node:fs');
const path = require('node:path');
const { stripTypeScriptTypes } = require('node:module');
const root = process.argv[2];
if (!root) throw new Error('Pinned Web checkout path required');
const source = fs.readFileSync(path.join(root, 'src/systems/avgun.ts'), 'utf8');
const code = stripTypeScriptTypes(source.replace(/^import .*;\s*$/mg, '')).replace(/\bexport /g, '');
const { AV_LOADOUT, avState, avTick, avFire, avReload } = new Function('tr', code + '\nreturn {AV_LOADOUT,avState,avTick,avFire,avReload};')((zh) => zh);
const kinds = ['zbl08', 'zbd05', 'ztd05', 'mengshi'];
const cases = [];
const snapshot = s => ({ selected: s.sel, guns: s.guns.map(g => ({ ready:g.ready, reserve:g.reserve, heat:g.heat, jammed:g.jam, reload_remaining:Math.max(0,g.reloadT), cooldown_remaining:g.cdT })) });
for (const kind of kinds) {
  for (const scenario of ['burst', 'partial_reload', 'cool', 'alternate']) {
    const s = avState(AV_LOADOUT[kind]);
    const steps = [];
    for(let i=0; i<240; i++) {
      const dt = 0.125;
      if(scenario==='partial_reload' && i===40) avReload(s);
      if(scenario==='alternate' && i===60) s.sel=(s.sel+1)%s.guns.length;
      const can = avTick(s,dt);
      const fired = can && (scenario!=='cool' || i<120);
      if(fired) avFire(s);
      if(i%20===0 || i===239) steps.push({index:i, fired, ...snapshot(s)});
    }
    cases.push({kind,scenario,steps});
  }
}
const result = {source_commit:'b9b0a2736642b37dd45925a4719dfef08b8f36b4',source:'src/systems/avgun.ts', loadouts:Object.fromEntries(kinds.map(k=>[k,AV_LOADOUT[k]])), cases};
fs.writeFileSync(path.join(__dirname, '../game/tests/fixtures/armor-reference.json'), JSON.stringify(result,null,2)+'\n');
console.log(`${cases.length} original mounted weapon scenarios`);
