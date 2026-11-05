#!/usr/bin/env python3
"""Run one fixed, read-only reviewer for this plan validation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path('/Users/xlyk/Code/ovrcr')
OUT = ROOT / 'research/workspace-session-ux-2026-11-05/plan-validation-omp'
MODEL = 'ollama-cloud/deepseek-v4.1-flash'
ROLES = ('feasibility', 'ux-lifecycle', 'acceptance')


def command(role, finalize=False):
    return [
        '/Users/xlyk/.local/bin/omp', '--model=' + MODEL,
        '--thinking=high', '--print', '--mode=json', '--no-session',
        '--no-title', '--no-prewalk', '--no-extensions', '--no-skills',
        '--no-rules', '--no-lsp', '--no-pty',
        '--no-tools' if finalize else '--tools=read,grep,glob',
        '--max-time=3m' if finalize else '--max-time=8m',
        '--append-system-prompt=' + str(OUT / 'reviewer-system.md'),
        '@' + str(OUT / (role + ('-finalize-prompt.md' if finalize else '-prompt.md'))),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('role', choices=ROLES)
    parser.add_argument('--finalize', action='store_true')
    args = parser.parse_args()
    env = os.environ.copy()
    for key in ('OVRCR_AGENT_SOCKET', 'OVRCR_AGENT_TOKEN'):
        env.pop(key, None)
    started = time.time()
    stamp = str(time.time_ns())
    phase = 'finalize' if args.finalize else 'scan'
    events = OUT / f'{args.role}-{phase}-{stamp}-events.jsonl'
    errors = OUT / f'{args.role}-{phase}-{stamp}-stderr.log'
    with events.open('x') as stdout, errors.open('x') as stderr:
        result = subprocess.run(command(args.role, args.finalize), cwd=ROOT, env=env,
                                stdout=stdout, stderr=stderr)
    receipt = dict(role=args.role, phase=phase, model=MODEL, exit_code=result.returncode,
                   elapsed_seconds=round(time.time() - started, 2),
                   events=str(events), stderr=str(errors))
    (OUT / f'{args.role}-{stamp}-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt), flush=True)
    return result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
