// A local, provider-free exercise of the real CLI and Host. Requests are saved for inspection/retry.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const [directoryArg,step]=process.argv.slice(2);
if(!directoryArg||!step) throw new Error('Usage: node examples/walkthrough.mjs DIRECTORY seed|inspect|correct|teach|explore|promote|commitment|decision|answer|mute|changes');
const directory=resolve(directoryArg), statePath=join(directory,'walkthrough.json');
const state=existsSync(statePath)?JSON.parse(readFileSync(statePath,'utf8')):{};
const save=()=>writeFileSync(statePath,JSON.stringify(state,null,2)+'\n',{mode:0o600});
const future=(minutes)=>new Date(Date.now()+minutes*60000).toISOString();
function cli(command,args=[],admin=false){
  const output=execFileSync(process.execPath,['--experimental-strip-types',join(root,'packages/cli/src/main.ts'),command,'--config',join(directory,admin?'admin.json':'client.json'),...args],{cwd:root,encoding:'utf8',stdio:['ignore','pipe','pipe']});
  return JSON.parse(output);
}
function call(name,request,admin=false){
  const path=join(directory,`${name}.request.json`);
  if(!existsSync(path)) writeFileSync(path,JSON.stringify(request,null,2)+'\n',{mode:0o600,flag:'wx'});
  const result=cli('call',['--file',path],admin);
  writeFileSync(join(directory,`${name}.response.json`),JSON.stringify(result,null,2)+'\n',{mode:0o600});
  return result;
}
const mutate=(name,mutation,admin=false)=>call(name,{action:'user',request:{action:'mutate',request_id:randomUUID(),mutation}},admin).result;
const record=(label,content)=>({label,scope:state.identity.scope,content,origin:'observed',evidential_status:'attributed_statement',availability:'routine',qualification:{status:'candidate'},valid_time:{kind:'unknown'},source_locators:[],derived_from:[],decision:{policy:state.policy.reference,action:'retain',reason:'User-supplied walkthrough evidence; not a verified building assessment',constraints:['Local demonstration only'],required_evidence:[],expires_at:null}});
const knowledge=(statement)=>({family:'knowledge',statement,subject:null,predicate:null,uncertainty:['No source file inspected in this exercise'],examined_coverage:['User statement only']});
let result;
switch(step){
case 'seed': {
  state.identity=cli('identity').result; save();
  state.policy=mutate('policy',{action:'create_policy',label:'Walkthrough retention',scope:state.identity.scope,effective:{kind:'unknown'},policy:{retention_purpose:'Local API walkthrough',allowed_uses:['task_context'],source_rules:[],evidence_requirements:['Attribute user statements'],applicability_rules:[],budget_class:'local',scheduling_priority:0,judgement_dispositions:[],notification_policy:'material',qualification_requirements:['Evaluate before procedure adoption']}},true).policy;save();
  const budgetPath=join(directory,'budget.request.json');
  const budget=existsSync(budgetPath)?JSON.parse(readFileSync(budgetPath,'utf8')).budget:{id:randomUUID(),scope:state.identity.scope,limit:{tokens:20000,provider_calls:10,output_bytes:100000,cost_microunits:0,sandbox_cpu_ms:0,sandbox_time_ms:0},final_result_reserve:{tokens:100,provider_calls:0,output_bytes:1000,cost_microunits:0,sandbox_cpu_ms:0,sandbox_time_ms:0},deadline:future(120),max_child_depth:0,max_child_concurrency:0,pricing_revision:'walkthrough/no-provider'};
  if(!state.budget){state.budget=call('budget',{action:'create_budget',budget},true).budget;save();}
  state.memory=mutate('remember',{action:'contribute',record:record('W-101 fire resistance',knowledge('The user reports W-101 as 60 minutes.'))}).memory;save();
  const fixture=JSON.parse(readFileSync(join(root,'evals/property-location/inputs/before-correction.json'),'utf8')).command;
  const taskScope={...state.identity.scope,task_id:randomUUID()};
  const brief={...fixture.payload,task_id:taskScope.task_id,scope:taskScope,inputs:{memories:[state.memory.reference],sources:[],artifacts:[]},capabilities:{sources:[],queries:[],tools:[]},evidence_cutoff:new Date().toISOString(),policy:state.policy.reference,profile:{id:randomUUID(),revision:1,label:'API walkthrough; no model worker attached'},limits:{root_budget_id:state.budget.id,max_provider_attempts:2,max_tokens:4096,max_output_bytes:8192,max_child_depth:0,max_child_concurrency:0}};
  state.job=call('submit',{action:'submit',command:{...fixture,request_id:randomUUID(),tenant_id:state.identity.tenant_id,actor_id:state.identity.actor_id,scope:taskScope,budget_id:state.budget.id,deadline:future(60),payload:{brief,parent_id:null,max_attempts:3,retain_until:future(1440)}}}).job;save();
  result={memory:state.memory.reference,job_id:state.job.id,scope:state.identity.scope};break;
}
case 'inspect':result={memories:cli('browse'),history:cli('history',['--id',state.memory.reference.memory_id]),task:cli('task',['--id',state.job.id])};break;
case 'correct': {
  const corrected=structuredClone(state.memory.record);corrected.content=knowledge('The user corrects W-101 to 90 minutes.');corrected.decision.action='revise';corrected.decision.reason='The user corrected the earlier statement; source verification remains open';
  result=mutate('correction',{action:'correct',expected:state.memory.reference,record:corrected});state.corrected=result.memory;save();break;
}
case 'teach':result=mutate('teach',{action:'contribute',record:record('How the property was found',{family:'episode',objective:'Locate the fire-resistance property',initial_conditions:['Instance lookup was empty'],observations:['Type lookup supplied the value'],actions:['Inspect instance then type properties'],corrections:['Do not infer absence from an empty instance lookup'],outcome:'Property location identified in a user demonstration',verification:[],uncertainty:['Demonstration has not been independently checked']})});break;
case 'explore': {
  state.exploration=mutate('open-exploration',{action:'open_exploration',scope:state.identity.scope,purpose:'Try a hypothetical alternative',expires_at:future(60)}).exploration;save();
  const hypothetical=record('Hypothesis: alternate wall type',knowledge('Suppose W-101 were replaced with a different wall type.'));
  state.exploration=mutate('add-exploration',{action:'add_exploration',id:state.exploration.id,expected_revision:state.exploration.revision,record:hypothetical}).exploration;save();result={exploration:state.exploration,collection:cli('browse')};break;
}
case 'promote':result=mutate('promote',{action:'promote_exploration',id:state.exploration.id,expected_revision:state.exploration.revision,index:0});break;
case 'commitment': {
  state.intention=mutate('commitment',{action:'contribute',record:record('Follow up the source check',{family:'intention',purpose:'Verify the reported 90 minutes against the source',owner_id:state.identity.actor_id,trigger:'When a source becomes available',readiness:['Source revision is available'],completion:['Source reference recorded'],expires_at:future(1440),notification_policy:'material',recurrence:null})}).memory;save();
  result=call('inspect-commitment',{action:'inspect_intentions',definition_id:state.intention.reference.memory_id});break;
}
case 'decision': {
  state.decision=mutate('request-decision',{action:'request_decision',job_id:state.job.id,owner_id:state.identity.actor_id,question:'Proceed with this user-supplied value for the local exercise?',missing:'Owner decision; no independently verified source is available',deadline:future(30)}).decisions[0];save();result=state.decision;break;
}
case 'answer':result=mutate('answer-decision',{action:'answer_decision',id:state.decision.id,expected_revision:state.decision.revision,answer:'approve',reason:'Proceed for the local exercise only; source verification is still required'});break;
case 'mute':result={preference:mutate('mute',{action:'notification_preference',mode:'muted'}),notifications:cli('notifications'),job:cli('task',['--id',state.job.id])};break;
case 'changes':result=cli('changes');break;
default:throw new Error(`Unknown step: ${step}`);
}
console.log(JSON.stringify(result,null,2));
