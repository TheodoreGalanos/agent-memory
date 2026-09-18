import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { expect, it } from 'vitest';
const exec=promisify(execFile);
it('runs the documented use case through the CLI and real authenticated Host',async()=>{
  const base=await mkdtemp(join(tmpdir(),'memory-walkthrough-')), directory=join(base,'demo');
  const socket=createServer();socket.listen(0,'127.0.0.1');await once(socket,'listening');
  const address=socket.address();if(!address||typeof address==='string')throw new Error('No port');const port=address.port;
  await new Promise<void>((resolve,reject)=>socket.close(e=>e?reject(e):resolve()));
  const runCli=async(command:string,...args:string[])=>JSON.parse((await exec(process.execPath,['--experimental-strip-types','packages/cli/src/main.ts',command,...args],{maxBuffer:4*1024*1024})).stdout);
  await runCli('init',directory,'--port',String(port));
  const host=spawn(resolve('target/debug/memory-host'),[join(directory,'host.json')],{stdio:['ignore','pipe','pipe']});
  let errors='';host.stderr.on('data',chunk=>{errors+=chunk});
  try {
    await Promise.race([once(host.stdout,'data'),once(host,'exit').then(()=>{throw new Error(errors)}),new Promise((_,reject)=>setTimeout(()=>reject(new Error('Host startup timeout')),10000).unref())]);
    const step=async(name:string)=>JSON.parse((await exec(process.execPath,['examples/walkthrough.mjs',directory,name],{maxBuffer:4*1024*1024})).stdout);
    const seed=await step('seed');expect(seed.memory.revision).toBe(1);
    const inspect=await step('inspect');expect(inspect.task.result.manifests).toEqual([]);
    expect(inspect.task.result.coverage.unexamined[0]).toContain('No render manifests');
    const correction=await step('correct');expect(correction.memory.reference.revision).toBe(2);
    const retried=await step('correct');expect(retried.memory.reference).toEqual(correction.memory.reference);
    const taught=await step('teach');expect(taught.memory.record.content.family).toBe('episode');expect(taught.memory.record.qualification.status).toBe('candidate');
    const explored=await step('explore');expect(explored.collection.result.records).toHaveLength(2);
    expect(explored.exploration.records[0].evidential_status).toBe('assumption');
    const promoted=await step('promote');expect(promoted.memory.record.evidential_status).toBe('assumption');
    const commitment=await step('commitment');expect(commitment.occurrences).toHaveLength(1);
    const decision=await step('decision');expect(decision.answer).toBeNull();
    const muted=await step('mute');expect(muted.notifications.result.page.events).toEqual([]);expect(muted.job.result.decisions[0].answer).toBeNull();
    const answered=await step('answer');expect(answered.decisions[0].answer).toBe('approve');
    const changes=await step('changes');expect(changes.result.revisions.length).toBeGreaterThan(0);
    const state=JSON.parse(await readFile(join(directory,'walkthrough.json'),'utf8'));
    const history=await runCli('history','--config',join(directory,'client.json'),'--id',state.memory.reference.memory_id);expect(history.result.records).toHaveLength(2);
    // Operator requests are unavailable to the same client that can contribute/correct.
    const forbidden=join(directory,'forbidden.json');await writeFile(forbidden,JSON.stringify({action:'set_control',kind:'provider',target:'fixture',expected_revision:0,enabled:false}));
    await expect(runCli('mutate','--config',join(directory,'client.json'),'--file',forbidden)).rejects.toThrow('403');
    // Restart verifies persisted state using the documented configuration.
    host.kill('SIGINT');await once(host,'exit');
    const restarted=spawn(resolve('target/debug/memory-host'),[join(directory,'host.json')],{stdio:['ignore','pipe','pipe']});
    try {await once(restarted.stdout,'data');const records=await runCli('browse','--config',join(directory,'client.json'));expect(records.result.records).toHaveLength(4);}
    finally {restarted.kill('SIGINT');await once(restarted,'exit');}
  } finally {if(host.exitCode===null){host.kill('SIGINT');await once(host,'exit');}await rm(base,{recursive:true,force:true});}
},60000);
