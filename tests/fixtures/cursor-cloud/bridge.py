#!/usr/bin/env python3
"""Local pinned-bridge fixture. Never contacts Cursor; streamed Send uses recorded SDK shapes."""
import argparse
import http.server
import json
import os
import socket
from pathlib import Path
import sys
import tempfile
import struct
import time
import threading

p = argparse.ArgumentParser()
p.add_argument('--host')
p.add_argument('--port', type=int)
p.add_argument('--workspace')
p.add_argument('--state-root')
a = p.parse_args()
root = Path(a.state_root).parent
# Fixture settings travel through the script's own adjacent file, not Bridge env.
config = json.loads(Path(sys.argv[0]).with_suffix('.json').read_text())
log = Path(config['log'])

def event(value):
    with log.open('a') as out:
        out.write(json.dumps(value) + '\n')

assert os.environ['CURSOR_API_KEY'] == 'ovrcr-fixture-not-a-credential'
assert not any(k.startswith('OVRCR_') for k in os.environ)
event({'event': 'started', 'pid': os.getpid(), 'adapter_pid': os.getppid(), 'root': str(root), 'environment_isolated': True})
token_dir = Path(tempfile.mkdtemp(prefix='cursor-sdk-bridge-', dir=os.environ['TMPDIR']))
token_file = token_dir / 'auth-token'
token_file.write_text('fixture_bridge_bearer_0123456789')
token_file.chmod(0o600)

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        method = self.path.rsplit('/', 1)[-1]
        raw = self.rfile.read(int(self.headers['Content-Length']))
        body = json.loads(raw[5:] if method in ('Send', 'ObserveRun') else raw)
        event({'event': 'rpc', 'method': method, **({'agent':body.get('agentId'),'run':body.get('runId')} if method == 'CancelRun' else {})})
        if self.headers.get('Authorization') != 'Bearer fixture_bridge_bearer_0123456789':
            self.send_error(401)
            return
        if config.get('reject_credential') and method == 'Me':
            self.send_error(401)
            return
        values = {'GetVersion': config.get('version_response', {'bridgeVersion': '1.0.0', 'protocolVersion': 'sdk.v1', 'capabilities': ['agent.create', 'agent.resume', 'agent.send', 'run.observe', 'run.wait', 'run.cancel', 'agent.management', 'cursor.catalog', 'artifacts.chunked', 'agent.usage']}),
                  'Me': {'user': {'userId': '123'}},
                  'ListModels': {'items': [{'id': config.get('model', 'composer-2.5')}]},
                  'ListRepositories': {'items': [{'url': config.get('repository', 'https://github.com/acme/app')}]}}
        if method == 'CreateAgent':
            expected = {'idempotencyKey': body.get('idempotencyKey'), 'options': {
                'apiKey': 'ovrcr-fixture-not-a-credential', 'model': {'id': 'composer-2.5'},
                'cloud': {'repos': [{'url': 'https://github.com/acme/app', 'startingRef': body.get('options',{}).get('cloud',{}).get('repos',[{}])[0].get('startingRef')}],
                'workOnCurrentBranch': False, 'autoCreatePr': False}}}
            assert body == expected and len(body['idempotencyKey']) == 64
            if config.get('drop_creation_reply'):
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            self.server.source_sha = body['options']['cloud']['repos'][0]['startingRef']
            self.server.creation_key = body['idempotencyKey']
            values[method] = {'agentId': 'fixture-agent-373', 'model': {'id': 'composer-2.5'}}
        if method == 'ObserveRun':
            if config.get('observe_scenario') == 'missing_run':
                self.send_error(404)
                return
            assert body['runId'] in self.server.streams
            offset = int(body.get('afterOffset', '0'))
            event({'event':'observe_request','run':body['runId'],'after':offset})
            marker = config.get('observe_pause_file')
            if marker:
                marker = Path(marker)
                marker.write_text(body['runId'])
                until = time.monotonic() + 30
                while not marker.with_suffix('.release').exists():
                    assert time.monotonic() < until
                    time.sleep(0.01)
            events = self.server.streams[body['runId']][offset:]
            scenario = config.get('observe_scenario')
            if scenario == 'wrong_run': events = [dict(offset='1',sdkMessage={'type':'thinking','message':{'agent_id':'fixture-agent-373','run_id':'foreign-run'}})]
            if scenario == 'changed_offset': events = events[:1] + [dict(events[1], offset='1')]
            if scenario == 'missing_offset': events = [{k:v for k,v in events[0].items() if k!='offset'}]
            if scenario == 'replayed_completion': events = events + events[-2:]
            if scenario == 'gap': events = [dict(events[0],offset='2')]
            drop = config.get('drop_observe_after') if offset == 0 or scenario == 'disconnect_forever' else None
            encoded = b''.join(struct.pack('>BI',0,len(item))+item for item in [json.dumps(v).encode() for v in events[:drop]])
            if drop is None: encoded += struct.pack('>BI',2,2)+b'{}'
            self.send_response(200)
            self.send_header('Content-Type','application/connect+json')
            self.send_header('Content-Length',str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)
            return
        if method == 'Send':
            assert set(body) == {'agentId', 'message', 'options', 'idempotencyKey'}
            assert body['agentId'] == 'fixture-agent-373' and body['options'] == {'model': {'id': 'composer-2.5'}}
            assert len(body['idempotencyKey']) == 64 and body['idempotencyKey'] != self.server.creation_key
            assert body['idempotencyKey'] not in self.server.send_keys
            self.server.send_keys.add(body['idempotencyKey'])
            self.server.sends += 1
            run = 'fixture-run-' + str(self.server.sends)
            self.server.run_ids.add(run)
            event({'event':'send_identity','agent':'fixture-agent-373','run':run,'key':body['idempotencyKey']})
            if config.get('drop_send_reply') or config.get('drop_send_reply_on') == self.server.sends:
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            if config.get('send_pause_file'):
                marker = Path(config['send_pause_file'])
                marker.write_text(run)
                until = time.monotonic() + 30
                while not marker.with_suffix('.release').exists():
                    assert time.monotonic() < until
                    time.sleep(0.01)
            def sdk(kind, **fields):
                return {'sdkMessage': {'type':kind, 'message':dict(agent_id='fixture-agent-373', run_id=run, **fields)}}
            if config.get('pause_after_identity'):
                first = json.dumps(sdk('thinking',text='')).encode()
                self.send_response(200)
                self.send_header('Content-Type','application/connect+json')
                self.end_headers()
                self.wfile.write(struct.pack('>BI',0,len(first))+first)
                self.wfile.flush()
                marker = Path(config['pause_after_identity'])
                marker.write_text(run)
                until = time.monotonic() + 30
                while not marker.with_suffix('.release').exists():
                    assert time.monotonic() < until
                    time.sleep(0.01)
            probe = dict(name='run_terminal_cmd' ,call_id='source-probe',args={'command':'git rev-parse HEAD'})
            result = {'agentId':'fixture-agent-373','runId':run,'status':'RUN_LIFECYCLE_STATUS_FINISHED','model':{'id':'composer-2.5'},'result':'Fixture inspection complete.'}
            if run in self.server.cancelled:
                result['status'] = config.get('cancel_outcome','RUN_LIFECYCLE_STATUS_CANCELLED')
            self.server.results[run] = result
            events = [sdk('thinking',text=''), sdk('tool_call',name='run_terminal_cmd',call_id='source-probe',status='running'),
                sdk('tool_call',**probe,status='completed',result={'success':{'command':'git rev-parse HEAD','stdout':self.server.source_sha+'\n','interleavedOutput':self.server.source_sha+'\n'},'isBackground':False}),
                sdk('assistant',message={'content':[{'type':'text','text':'Inspecting selected source.\n'}]}),
                {'result':dict(agentId='fixture-agent-373',runId=run,status=result['status'],result=result)},
                {'done':{'agentId':'fixture-agent-373','runId':run}}]
            scenario = config.get('send_scenario')
            if scenario == 'cross_run': events[1]['sdkMessage']['message']['run_id'] = 'foreign-run'
            if scenario == 'unknown_shape': events[1]['sdkMessage']['type'] = 'unrecognized'
            if scenario == 'bad_source': events[2]['sdkMessage']['message']['result']['success']['stdout'] = 'wrong\n'
            if scenario == 'bad_call': events[2]['sdkMessage']['message']['call_id'] = 'different-call'
            if scenario in ('shell_success', 'shell_flat_success'):
                events[1] = sdk('tool_call',name='shell',call_id='source-probe',args={'command':'git rev-parse HEAD'},status='running')
                success = {'stdout':self.server.source_sha+'\n','stderr':'','exitCode':0}
                result_value = success if scenario == 'shell_flat_success' else {'status':'success','value':dict(success,signal='')}
                events[2] = sdk('tool_call',name='shell',call_id='source-probe',args={'command':'git rev-parse HEAD'},status='completed',result=result_value)
            if scenario == 'shell_failure_discriminator':
                events[1] = sdk('tool_call',name='shell',call_id='source-probe',args={'command':'git rev-parse HEAD'},status='running')
                events[2] = sdk('tool_call',name='shell',call_id='source-probe',args={'command':'git rev-parse HEAD'},status='completed',result={'status':'failure','exitCode':0,'stderr':'','stdout':self.server.source_sha+'\n'})
            if scenario == 'bad_success': events[2]['sdkMessage']['message']['result']['success']['exitCode'] = 1
            if scenario == 'missing_done': events.pop()
            if scenario == 'terminal_replay': events.append(events[-2])
            if config.get('result_branch'):
                result['git'] = {'branches':[{'repoUrl':'https://github.com/acme/app','branch':'fixture-result','prUrl':'https://github.com/acme/app/pull/17' if config.get('result_pr') else ''}]}
            if config.get('equal_content_events'): events.insert(4,events[3])
            self.server.streams[run] = [dict(v, offset=str(i+1)) for i,v in enumerate(events)]
            if scenario == 'duplicate':
                events = list(self.server.streams[run])
                events.insert(4,events[3])
            drop = config.get('drop_send_after')
            if drop is not None:
                events = [dict(v, offset=str(100+i)) for i,v in enumerate(events[:drop])]
            encoded = b''.join(struct.pack('>BI',0,len(item))+item for item in [json.dumps(v).encode() for v in events])
            if drop is None: encoded += struct.pack('>BI',2,2)+b'{}'
            if not config.get('pause_after_identity'):
                self.send_response(200)
                self.send_header('Content-Type','application/connect+json')
                self.send_header('Content-Length',str(len(encoded)))
                self.end_headers()
            self.wfile.write(encoded)
            return
        if method == 'CancelRun':
            assert body == {'agentId':'fixture-agent-373','runId':body['runId']} and body['runId'] in self.server.run_ids
            self.server.cancelled.add(body['runId'])
            values[method] = {}
        if method == 'GetAgent':
            assert body == {'agentId':'fixture-agent-373','options':{'apiKey':'ovrcr-fixture-not-a-credential'}}
            repo = 'https://github.com/other/app' if config.get('send_scenario') == 'wrong_repository' else 'https://github.com/acme/app'
            values[method] = {'agent':{'agentId':'fixture-agent-373','cloud':{'repos':[repo]}}}
        if method == 'GetRun':
            if config.get('recovery_missing'):
                self.send_error(404)
                return
            assert body['options'] == {'runtime':'RUNTIME_CLOUD','agentId':'fixture-agent-373','apiKey':'ovrcr-fixture-not-a-credential'}
            if body['runId'] in config.get('recovery_runs', {}):
                self.server.run_ids.add(body['runId'])
                self.server.results[body['runId']] = dict(agentId='fixture-agent-373',runId=body['runId'],status=config['recovery_runs'][body['runId']])
            assert body['runId'] in self.server.run_ids
            values[method] = {'run':self.server.results[body['runId']]}
            if config.get('cancel_snapshot_outcome') and body['runId'] in self.server.cancelled:
                values[method]['run'] = dict(values[method]['run'],status=config['cancel_snapshot_outcome'])
            if config.get('send_scenario') == 'wrong_snapshot': values[method]['run'] = dict(values[method]['run'],runId='foreign-run')
        if method not in values:
            self.send_error(405)
            return
        if method not in ['GetVersion', 'CreateAgent', 'GetAgent', 'GetRun', 'CancelRun'] and body != {'options': {'apiKey': 'ovrcr-fixture-not-a-credential'}}:
            self.send_error(401)
            return
        encoded = json.dumps(values[method]).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
server.streams = {}
server.send_keys = set()
server.sends = 0
server.run_ids = set()
server.results = {}
server.cancelled = set()
port = server.server_port
print('cursor-sdk-bridge ready ' + json.dumps({'schemaVersion': 1, 'serverVersion': '1.0.0', 'pid': os.getpid(),
    'transport': 'tcp', 'protocol': 'connect', 'host': '127.0.0.1', 'port': port,
    'url': 'http://127.0.0.1:' + str(port), 'workspaceRef': a.workspace,
    'stateRoot': a.state_root, 'authTokenFile': str(token_file)}), file=sys.stderr, flush=True)
server.serve_forever()
