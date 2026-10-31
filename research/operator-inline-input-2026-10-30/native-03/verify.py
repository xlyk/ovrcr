"""Verify retained native03 receipts only; never open a GUI or a live fixture."""
import gzip,hashlib,json,pathlib
p=pathlib.Path(__file__).resolve().parent
read=lambda name:json.loads((p/(name+'.json')).read_text())
ax=lambda name:gzip.decompress((p/(name+'.ax.txt.gz')).read_bytes()).decode()
state=read('state');repo=p.parents[2]
assert state['source_revision']=='8a141c71c0e3ccd6928b463ac24d6b9eb66ffff2'
for path,digest in state['source_sha256'].items(): assert hashlib.sha256((repo/path).read_bytes()).hexdigest()==digest,path
phases=['full-before-owned','full-after','quick-after','quick-empty','full-clear','full-empty-mixed']
first=read(phases[0]+'-boundary')
for phase in phases:
    current=read(phase+'-boundary')
    for field in ['operator_conversations','operator_turns','operator_messages','control_markers','frozen_sha256']:
        assert current[field]==first[field],(phase,field)
    assert {row['conversation']:row['state'] for row in current['operator_turns']}=={1:'Running',2:'Running'}
assert len(first['operator_conversations'])==2 and len(first['operator_messages'])==10
for name in ['01-full-before-mixed','02-full-after-mixed']:
    assert '> Full ▏preserved 界🙂 tail' in ax(name) and 'second café' in ax(name) and 'Responding' in ax(name)
for name in ['03-quick-before-mixed','04-quick-after-mixed']:
    assert 'Quick ▏preserved 界🙂 tail' in ax(name) and '> Full ▏preserved 界🙂 tail' in ax(name) and 'second café' in ax(name)
assert 'Quick message' in ax('05-quick-cleared-empty-no-stop') and 'Quick ▏preserved' not in ax('05-quick-cleared-empty-no-stop')
assert 'Full ▏preserved' not in ax('06-full-cleared-still-running') and 'fixture-a · running' in ax('06-full-cleared-still-running')
states=lambda phase:{row['conversation']:row['state'] for row in read(phase+'-boundary')['operator_turns']}
assert states('selected-stop')=={1:'Interrupted',2:'Running'}
assert states('terminal-after')=={1:'Interrupted',2:'Running'}
assert states('final')=={1:'Interrupted',2:'Complete'}
rows=lambda phase:read(phase+'-processes')
worker=lambda phase,conversation:[row for row in rows(phase) if 'operator worker --conversation '+str(conversation)+' ' in row['command']]
assert len(worker('full-before-owned',1))==len(worker('full-before-owned',2))==1
assert not worker('selected-stop',1) and len(worker('selected-stop',2))==1
assert len(worker('terminal-after',2))==1 and not worker('final',2)
sleeps=[row for row in rows('terminal-before') if row['command']=='/bin/sleep 30'];assert len(sleeps)==1
assert not any(row['pid']==sleeps[0]['pid'] for row in rows('terminal-after'))
assert any(row['pid']==83249 for row in rows('terminal-after'))
assert 'NATIVE3_TERMINAL_SURVIVED' in ax('10-terminal-ctrlc-survived')
assert read('10-terminal-ctrlc-survived')['end_unix_ms']-read('terminal-before-boundary')['unix_ms']<30000
assert 'B retained independent draft 界🙂' in ax('08-independent-b-survives') and 'B retained independent draft 界🙂' in ax('11-independent-b-completed-draft')
assert 'B final' in ax('11-independent-b-completed-draft') and 'Responding' not in ax('11-independent-b-completed-draft')
assert read('provider-finished')=={'seeded':True,'a_requests':2,'b_requests':2,'quick_requests':0,'activity_requests':0}
images=list(p.glob('*.jpg'));assert len(images)==11
for image in images:
    data=image.read_bytes();text=gzip.decompress(image.with_suffix('.ax.txt.gz').read_bytes());meta=read(image.stem)
    assert data[:3]==b'\xff\xd8\xff' and hashlib.sha256(data).hexdigest()==meta['jpg_sha256']
    assert hashlib.sha256(text).hexdigest()==meta['ax_sha256'] and meta['source_revision']==state['source_revision']
for owner in ['gui','holder']:
    assert read(owner+'-exit')['original_wait_exit']==0
    raw=gzip.decompress((p/(owner+'.log.gz')).read_bytes());meta=read(owner+'-log-meta')
    assert hashlib.sha256(raw).hexdigest()==meta['original_sha256'] and len(raw)==meta['original_bytes']
cleanup=read('cleanup')
assert all(value=='absent' for field in ['pids','pgids'] for value in cleanup[field].values())
assert cleanup['root_removed'] and cleanup['hashes_unchanged'] and not cleanup['demo_exists'] and not cleanup['socket_exists']
result={'result':'passed','captures':len(images),'pids_absent':len(cleanup['pids']),'groups_absent':len(cleanup['pgids']),'source_revision':state['source_revision']}
(p/'verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
