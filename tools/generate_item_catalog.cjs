// Regenerate from web commit b9b0a2736642b37dd45925a4719dfef08b8f36b4 using Node 24, then cargo fmt.
const fs = require('node:fs');
const {stripTypeScriptTypes} = require('node:module');
const path = require('node:path');
const webRoot = process.argv[2];
if (!webRoot) throw new Error('Usage: node tools/generate_item_catalog.cjs /path/to/defendtaipei-web');
const src = fs.readFileSync(path.join(webRoot, 'src/systems/items.ts'),'utf8');
const js = stripTypeScriptTypes(src.replace(/^import[^\n]+\n/,'').replace(/\bexport /g,''));
const {ITEMS,LOOT} = new Function('tr',js+';return {ITEMS,LOOT};')((zh,en)=>({zh,en}));
const out = ['//! Item definitions ported from web v24.42.0 `src/systems/items.ts`.', '// Original source: b9b0a2736642b37dd45925a4719dfef08b8f36b4.', 'use super::{ItemDef, ItemKind, LootEntry, LootTable};', '', 'pub const ITEMS: &[ItemDef] = &['];
const float = n=>Number.isInteger(n)?`${n}.0`:String(n);
const strings = {ammo:'ammo',cook:'cook',cal:'cal'};
const nums={w:'weight',hunger:'hunger',thirst:'thirst',fatigue:'fatigue',hp:'hp',dmg:'damage',range:'range',noise:'noise',cd:'cooldown',risk:'risk',at:'anti_armor'};
const bools={raw:'raw',perish:'perish',stopBleed:'stop_bleed',roc:'roc'};
for(const [id,d] of Object.entries(ITEMS)){
 out.push('    ItemDef {',`        id: ${JSON.stringify(id)},`,`        name_zh: ${JSON.stringify(d.name.zh)},`,`        name_en: ${JSON.stringify(d.name.en)},`,`        kind: ItemKind::${d.kind[0].toUpperCase()+d.kind.slice(1)},`);
 for(const [old,k] of Object.entries(nums))if(d[old]!==undefined)out.push(`        ${k}: ${float(d[old])},`);
 for(const [old,k] of Object.entries(strings))if(d[old]!==undefined){if(typeof d[old]==='object'){out.push(`        ${k}: Some(${JSON.stringify(d[old].en)}),`);out.push(`        cal_zh: Some(${JSON.stringify(d[old].zh)}),`);}else out.push(`        ${k}: Some(${JSON.stringify(d[old])}),`);}
 for(const [old,k] of Object.entries(bools))if(d[old]!==undefined)out.push(`        ${k}: ${d[old]},`);
 for(const k of ['cap','mag'])if(d[k]!==undefined)out.push(`        ${k}: ${d[k]},`);
 if(d.desc){out.push(`        desc_zh: ${JSON.stringify(d.desc.zh)},`,`        desc_en: ${JSON.stringify(d.desc.en)},`);}
 out.push('        ..ItemDef::DEFAULT','    },');
}
out.push('];','','/// Original [item, selection weight, maximum count] loot tables.','pub const LOOT: &[LootTable] = &[');
for(const [kind,entries] of Object.entries(LOOT)){
 out.push('    LootTable {',`        kind: ${JSON.stringify(kind)},`,'        entries: &[');
 for(const [id,weight,max] of entries)out.push(`            LootEntry { item: ${JSON.stringify(id)}, weight: ${float(weight)}, max: ${max} },`);
 out.push('        ],','    },');
}
out.push('];','');
const dir=path.resolve(__dirname, '../game/src/inventory');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(dir+'/catalog.rs',out.join('\n'));
console.log(`${Object.keys(ITEMS).length} item definitions, ${Object.keys(LOOT).length} loot tables written`);
