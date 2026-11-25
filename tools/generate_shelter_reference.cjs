// Regenerate from web commit b9b0a2736642b37dd45925a4719dfef08b8f36b4 using Node 24.
const fs=require('node:fs');
const {stripTypeScriptTypes}=require('node:module');
const path=require('node:path');
const webRoot=process.argv[2];
if(!webRoot) throw new Error('Usage: node tools/generate_shelter_reference.cjs /path/to/defendtaipei-web');
const source=fs.readFileSync(path.join(webRoot, 'src/systems/survival.ts'),'utf8');
const js=stripTypeScriptTypes(source.replace(/^import[^\n]+\n/,'').replace(/\bexport /g,''));
const api=new Function('tr',js+';return {newSurv,dayTick,pray,endingText,price};')((zh,en)=>en);
const clean=s=>s.replace(/[💀🔦🚪⚡🚱📢🌀🍚📻☁]\s*/gu,'');
const skills={cook:'Cook',pray:'Pray',build:'Build',scav:'Scavenge'};
const upg={boards:'Boards',beds:'Beds',stove:'Stove',rain:'Rain',planter:'Planter'};
function snapshot(s,env){return {shelter:{day:s.day,karma:s.karma,scarcity:s.scarcity,upgrades:Object.keys(s.upg).map(k=>upg[k]),group:s.group.map(g=>({id:g.id,skill:skills[g.skill],hunger:g.hunger,sick:g.sick,hurt:g.hurt,mood:g.mood,here:g.here,dead:!!g.dead})),stash:Object.entries(s.stash),event:clean(s.ev),log:s.log.map(clean),plant_day:s.plantDay,pray_day:s.prayDay,raided:s.raided,stole:s.stole,gave:s.gave,home:null},env:{...env}};}
const scenarios=[
{name:'unassisted_30_days',days:30,setup:s=>{}},
{name:'stocked_upgrades_rain',days:12,raining:true,setup:s=>{s.upg={boards:1,beds:1,stove:1,rain:1,planter:1};s.stash={rice:2,water:80,rawrice:40,pill:20};s.group.forEach((g,i)=>{g.here=true;g.sick=31+i*10;g.hurt=20+i*5;g.mood=40+i*5;});}},
{name:'deaths_then_arrival',days:8,setup:s=>{s.karma=-4;s.stash={water:0,rice:0,pill:1};s.group[0].sick=110;s.group[0].hunger=0;s.group[0].mood=2;s.group[3].sick=94;s.group[3].hunger=1;s.group[3].mood=3;}},
{name:'prayer_and_ordered_raid',days:12,pray:true,setup:s=>{s.karma=6;s.stash={water:2,rice:2,can:40,boiled:60};s.upg={planter:1};}},
];
const cases=scenarios.map(spec=>{const s=api.newSurv();spec.setup(s);const env={raining:!!spec.raining,power:true,water:true};const initial=snapshot(s,env);const dawns=[];for(let i=0;i<spec.days;i++){if(spec.pray)api.pray(s);api.dayTick(s,env.raining,{power:on=>env.power=on,water:on=>env.water=on});dawns.push(snapshot(s,env));}return {name:spec.name,pray:!!spec.pray,initial,dawns,ending:clean(api.endingText(s))};});
const prices=[1,1.14,1.5,2,3].map(scarcity=>{const s=api.newSurv();s.scarcity=scarcity;return{scarcity,values:Object.fromEntries(['cig','umbrella','poncho','mask','powerbank','bread','battery','can','rice','noodle','pill','bandage','medkit','water','plank','scrap','cloth','seeds','gas','rawrice','butane','cstove','mianxian','peanutcan','roufong','cracker','canfruit','driedtofu','boiled','rainwater','cupnoodle','unknown'].map(k=>[k,api.price(s,k)]))};});
const target=path.resolve(__dirname, '../game/tests/fixtures');fs.mkdirSync(target,{recursive:true});
fs.writeFileSync(target+'/shelter-reference.json',JSON.stringify({source:'lijiabao1998/defendtaipei@b9b0a2736642b37dd45925a4719dfef08b8f36b4',file:'src/systems/survival.ts',note:'Executed unmodified original functions with English tr, using Node TypeScript type erasure. Only decorative emoji stripped for native font compatibility.',cases,prices},null,2)+'\n');
console.log(cases.length+' scenarios, '+cases.reduce((n,c)=>n+c.dawns.length,0)+' dawns, '+prices.length+' price bands');
