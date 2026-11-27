// Execute the pinned Web drive.ts numerical rules with renderer-only deps stubbed.
// Usage: node tools/generate_vehicle_reference.cjs /path/to/defendtaipei-web
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { stripTypeScriptTypes } = require('node:module');
const root = process.argv[2];
if (!root) throw new Error('A pinned Web checkout is required.');
const commit = 'b9b0a2736642b37dd45925a4719dfef08b8f36b4';
if (execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== commit) throw new Error('Wrong Web reference commit.');
const source = fs.readFileSync(path.join(root, 'src/systems/drive.ts'), 'utf8');
const js = stripTypeScriptTypes(source.replace(/^import[^\n]*\n/gm, '').replace(/\bexport /g, ''), { mode: 'transform' });
const THREE = { MathUtils: { clamp: (n,a,b) => Math.max(a,Math.min(b,n)), lerp: (a,b,t) => a+(b-a)*t } };
const api = new Function('tr','THREE','HELI_NAME',js+';return {SPEC,DriveSystem};')((zh,en)=>en,THREE,{uh60:'UH-60'});
const spec = Object.fromEntries(Object.entries(api.SPEC).filter(([kind])=>kind!=='uh60'));
const cases = [
  {name:'scooter_accelerate_turn_brake',kind:'scooter',ride:1,initial:{fuel:3.2,eng:.8,tyre:1,hd:0,v:0},steps:[{dt:.1,want:[0,1],repeat:10},{dt:.1,want:[1,0],repeat:10},{dt:.1,want:[0,0],repeat:10}]},
  {name:'courier_damaged_tyre',kind:'delivery',ride:1.25,initial:{fuel:4,eng:1,tyre:.3,hd:0,v:0},steps:[{dt:.1,want:[0,1],repeat:50}]},
  {name:'sedan_reverse',kind:'sedan',ride:1,initial:{fuel:20,eng:1,tyre:1,hd:0,v:0},steps:[{dt:.1,want:[0,-1],repeat:15}]},
  {name:'fuel_empty_coast',kind:'moto',ride:1,initial:{fuel:0,eng:.8,tyre:1,hd:0,v:10},steps:[{dt:.1,want:[0,1],repeat:10}]},
  {name:'high_speed_impact',kind:'sedan',ride:1,initial:{fuel:20,eng:1,tyre:1,hd:0,v:12},blocked:true,steps:[{dt:.05,want:[0,1],repeat:1}]},
  {name:'pedal_no_fuel',kind:'basket',ride:1,initial:{fuel:0,eng:.7,tyre:1,hd:0,v:0},steps:[{dt:.1,want:[0,1],repeat:35}]},
];
for (const scenario of cases) {
  const deps = {scene:{},skills:()=>({ride:scenario.ride}),noise:()=>{},log:()=>{},armor:()=>[],hurt:()=>{},roadW:()=>12,power:()=>true};
  const system = new api.DriveSystem(deps);
  const pos={x:0,y:0,z:0};
  const v={id:0,kind:scenario.kind,s:api.SPEC[scenario.kind],pos,g:{position:pos,rotation:{x:0,y:0,z:0}},wheels:[],bat:1,key:true,...scenario.initial};
  system.list=[v];system.cur=v;system.blocked=()=>!!scenario.blocked;
  scenario.snapshots=[];
  for (const step of scenario.steps) for(let i=0;i<step.repeat;i++) {
    const want={x:step.want[0],y:0,z:step.want[1],length:()=>Math.hypot(...step.want)};
    system.update(step.dt,want,true);
    scenario.snapshots.push({pos:[pos.x,pos.z],heading:v.hd,speed:v.v,fuel:v.fuel,engine:v.eng,tyre:v.tyre});
  }
}
const out={source:'lijiabao1998/defendtaipei@'+commit,file:'src/systems/drive.ts',sha256:crypto.createHash('sha256').update(source).digest('hex'),note:'Original SPEC and DriveSystem.update executed with only renderer/dependency mocks. Native collision substeps, deterministic placement and bounded inventory transactions are separately tested.',spec,cases};
const target=path.resolve(__dirname,'../game/tests/fixtures/vehicle-reference.json');
fs.writeFileSync(target,JSON.stringify(out,null,2)+'\n');
console.log(Object.keys(spec).length+' ground specifications and '+cases.reduce((n,c)=>n+c.snapshots.length,0)+' original driving snapshots');
