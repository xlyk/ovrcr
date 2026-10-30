"""Post-wait kernel/filesystem verification; never query or relaunch the closed GUI."""
import hashlib,json,os,pathlib,shutil,time
p=pathlib.Path(__file__).resolve().parent
state=json.loads((p/'state.json').read_text()); root=pathlib.Path(state['root'])
assert root.parent == pathlib.Path('/private/tmp') and root.name.startswith('ovrcr-inline-native3-')
for owner in ['holder','gui']:
    assert json.loads((p/(owner+'-exit.json')).read_text())['original_wait_exit'] == 0
rows=[row for path in p.glob('*-processes.json') for row in json.loads(path.read_text())]
pids={row['pid'] for row in rows} | {json.loads((p/(owner+'-owner.json')).read_text())['pid'] for owner in ['gui','holder']}
groups={row['pgid'] for row in rows}
def absent(identifier):
    try: os.kill(identifier,0)
    except ProcessLookupError: return 'absent'
    except PermissionError: return 'permission-denied'
    return 'present'
r={'unix_ms':int(time.time()*1000),'pids':{str(pid):absent(pid) for pid in sorted(pids)},'pgids':{str(group):absent(-group) for group in sorted(groups)},'demo_exists':pathlib.Path(state['demo']).exists(),'socket_exists':(pathlib.Path(state['demo'])/'server.sock').exists()}
r['hashes_unchanged']=all(hashlib.sha256((pathlib.Path(state['bundle'])/'Contents/MacOS'/name).read_bytes()).hexdigest()==digest for name,digest in state['sha256'].items())
assert all(v=='absent' for field in ['pids','pgids'] for v in r[field].values()), r
assert r['hashes_unchanged'] and not r['demo_exists'] and not r['socket_exists'], r
shutil.rmtree(root);r['root_removed']=not root.exists()
(p/'cleanup.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2))
