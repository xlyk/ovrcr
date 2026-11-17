# Controlled provider's read-only native Bash step; never an OVRCR implementation.
import json
import os
import subprocess

cli = os.environ['OVRCR_EXECUTABLE']

def get(*args):
    return json.loads(subprocess.check_output([cli, '--json', *args], text=True))

def git(path, *args):
    result = subprocess.run(['git', '--no-optional-locks', '-C', path, *args],
                            capture_output=True, text=True)
    return {'code': result.returncode, 'text': result.stdout.strip()}

projects = get('project', 'list')
workspaces = get('workspace', 'list')
for workspace in workspaces:
    path = workspace['path']
    workspace['git'] = {
        'dirty': git(path, 'status', '--porcelain=v1', '--untracked-files=all'),
        'head': git(path, 'rev-parse', 'HEAD'),
        'branch': git(path, 'branch', '--show-current'),
        'unmerged': git(path, 'log', '--oneline', 'main..HEAD'),
        'unpublished': git(path, 'log', '--oneline', 'HEAD', '--not', '--remotes'),
    }
print('CLEANUP_INVENTORY=' + json.dumps({
    'executable': cli, 'instance': os.environ['OVRCR_HOME'], 'projects': projects,
    'workspaces': workspaces, 'sessions': get('terminal', 'list'),
    'archived': get('terminal', 'list', '--archived'), 'runs': get('run', 'list')}))
