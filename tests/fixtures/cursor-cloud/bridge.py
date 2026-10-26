#!/usr/bin/env python3
"""Local catalog fixture; any mutation RPC is logged and rejected."""
import argparse
import http.server
import json
import os
from pathlib import Path
import sys
import tempfile

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
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        event({'event': 'rpc', 'method': method})
        if self.headers.get('Authorization') != 'Bearer fixture_bridge_bearer_0123456789':
            self.send_error(401)
            return
        if config.get('reject_credential') and method == 'Me':
            self.send_error(401)
            return
        values = {'GetVersion': {'bridgeVersion': '1.0.0', 'protocolVersion': 'sdk.v1'},
                  'Me': {'user': {'userId': '123'}},
                  'ListModels': {'items': [{'id': config.get('model', 'composer-2.5')}]},
                  'ListRepositories': {'items': [{'url': config.get('repository', 'https://github.com/acme/app')}]}}
        if method not in values:
            self.send_error(405)
            return
        if method != 'GetVersion' and body != {'options': {'apiKey': 'ovrcr-fixture-not-a-credential'}}:
            self.send_error(401)
            return
        encoded = json.dumps(values[method]).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
port = server.server_port
print('cursor-sdk-bridge ready ' + json.dumps({'schemaVersion': 1, 'serverVersion': '1.0.0', 'pid': os.getpid(),
    'transport': 'tcp', 'protocol': 'connect', 'host': '127.0.0.1', 'port': port,
    'url': 'http://127.0.0.1:' + str(port), 'workspaceRef': a.workspace,
    'stateRoot': a.state_root, 'authTokenFile': str(token_file)}), file=sys.stderr, flush=True)
server.serve_forever()
