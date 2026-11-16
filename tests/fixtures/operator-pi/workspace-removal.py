# Controlled provider plan executed by installed Pi's actual Bash tool.
import json
import os
import subprocess
import sys

exe = os.environ['OVRCR_EXECUTABLE']
action = json.loads(sys.argv[1])

def cli(*args):
    return subprocess.run([exe, '--json', *args], text=True, capture_output=True)

def read(*args):
    result = cli(*args)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)

results = []
for target in action['targets']:
    if action['mode'] == 'preview':
        response = cli('workspace', 'removal-preview', '--project', target['project'], '--path', target['path'])
        item = {'target': target, 'exit': response.returncode, 'stderr': response.stderr}
        if response.returncode == 0:
            item['expected'] = json.loads(response.stdout)
            for label, revs in [('unmerged', ['main..HEAD']), ('unpublished', ['HEAD', '--not', '--remotes'])]:
                git = subprocess.run(['git', '-C', target['path'], 'log', '--oneline', *revs], text=True, capture_output=True)
                item[label] = git.stdout if git.returncode == 0 else 'unknown'
        results.append(item)
    else:
        response = cli('workspace', 'remove-approved', '--expected', json.dumps(target))
        results.append({'expected': target, 'exit': response.returncode, 'stderr': response.stderr,
                        'path_exists': os.path.exists(target['workspace']['path'])})
print('WORKSPACE_CLEANUP=' + json.dumps({'results': results, 'workspaces': read('workspace', 'list'),
                                       'archived': read('terminal', 'list', '--archived')}))
