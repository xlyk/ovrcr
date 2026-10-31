import hashlib,json,os,pathlib,subprocess,sys,gzip
repo=pathlib.Path(__file__).resolve().parents[2]
evidence=pathlib.Path(__file__).resolve().parent
phase=sys.argv[1]
state = json.loads((evidence / 'state.json').read_text())
root = pathlib.Path(state['root'])
env = {'PATH':'/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin', 'HOME':str(root/'home'), 'TMPDIR':str(root/'temp'), 'SHELL':'/bin/zsh', 'NO_PROXY':'*', 'no_proxy':'*', 'GIT_CONFIG_GLOBAL':str(root/'gitconfig'), 'GIT_CONFIG_NOSYSTEM':'1'}
if phase == 'holder':
    env.update(OVRCR_OPERATOR_E2E_HOLDER=str(root/'control'), OVRCR_OPERATOR_E2E_DEMO_PARENT=str(root/'temp'))
    command = [state['holder'], '--exact', 'compiled_worker::native_provider_holder', '--ignored', '--nocapture', '--test-threads=1']
elif phase == 'gui':
    env['OVRCR_OPERATOR_TEST_API_BASE'] = json.loads((root/'control/base.json').read_text())['api_base']
    command = [str(pathlib.Path(state['bundle'])/'Contents/MacOS/ovrcr-gui')]
else:
    raise SystemExit('unknown phase')
with (evidence/(phase+'.log.gz')).open('wb') as raw, gzip.GzipFile(filename='',mode='wb',fileobj=raw,mtime=0) as log:
    child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (evidence/(phase+'-owner.json')).write_text(json.dumps({'pid':child.pid, 'command':command, 'root':str(root)}, indent=2)+'\n')
    print(phase, 'pid', child.pid, flush=True)
    digest=hashlib.sha256(); size=0
    while chunk := child.stdout.read(65536):
        digest.update(chunk); size+=len(chunk); log.write(chunk); log.flush()
    result = child.wait()
    (evidence/(phase+'-log-meta.json')).write_text(json.dumps({'original_sha256':digest.hexdigest(),'original_bytes':size},indent=2)+'\n')
(evidence/(phase+'-exit.json')).write_text(json.dumps({'original_wait_exit':result}, indent=2)+'\n')
print(phase, 'original wait', result, flush=True)
sys.exit(result)
