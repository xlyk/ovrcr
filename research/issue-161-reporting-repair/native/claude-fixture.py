#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
if sys.argv[1:] == ['--version']:
    print('2.1.267 (Claude Code)')
    raise SystemExit(0)
binary='/Users/xlyk/Documents/Codex/2026-09-30/task-4/issue-161/target/OVRCR GUI.app/Contents/MacOS/ovrcr'
conversation=sys.argv[sys.argv.index('--resume')+1]
transcript=pathlib.Path(__file__).parent/'owned-transcript.jsonl'
real={'type':'assistant','sessionId':conversation,'isSidechain':False,'requestId':'fixture-request','message':{'id':'fixture-api','model':'fixture-model','usage':{'input_tokens':10,'cache_read_input_tokens':0,'cache_creation_input_tokens':0,'output_tokens':1}}}
synthetic={'type':'assistant','sessionId':conversation,'isSidechain':False,'message':{'id':'interruption','model':'<synthetic>','usage':None}}
transcript.write_text(json.dumps(real)+'\n'+json.dumps(synthetic)+'\n')
def hook(event, **fields):
    payload={'hook_event_name':event,'session_id':conversation,'agent_type':'fixture-root',**fields}
    result=subprocess.run([binary,'report','claude','--stdin-json'],input=json.dumps(payload),text=True,capture_output=True)
    if result.returncode: raise RuntimeError('owned fixture callback failed')
hook('SessionStart',source='resume',transcript_path=str(transcript))
print('OWNED SYNTHETIC CLAUDE FIXTURE. Commands: ready, freeze, doctor, ping, quit.',flush=True)
for line in sys.stdin:
    command=line.strip()
    if command=='ready':
        hook('UserPromptSubmit',prompt_id='native-gui-fixture-turn')
        hook('Stop',prompt_id='native-gui-fixture-turn',stop_hook_active=False)
        print('FIXTURE_READY_HANDLED',flush=True)
    elif command=='freeze':
        hook('SessionStart',source='fork',session_id='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',transcript_path=str(transcript))
        print('FIXTURE_FREEZE_HANDLED',flush=True)
    elif command=='doctor':
        result=subprocess.run([binary,'agent','doctor','claude','--json','--session',(pathlib.Path(__file__).parent/'public-session-id').read_text().strip(),'--executable',str(pathlib.Path(__file__).resolve())],capture_output=True,text=True)
        report=json.loads(result.stdout)
        print('DOCTOR_STATUS='+report['session_status'],flush=True)
        print('DOCTOR_REASON='+str(report['source_health']['reason']),flush=True)
        print(next((s for s in report['remediation'] if 'foreground conversation' in s),'No identity warning'),flush=True)
    elif command=='ping': print('CUA_NATIVE_INPUT_OK',flush=True)
    elif command=='quit': break
    else: print('UNKNOWN_FIXTURE_COMMAND',flush=True)
