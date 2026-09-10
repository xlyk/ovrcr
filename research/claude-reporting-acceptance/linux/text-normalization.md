# Linux evidence text normalization

On 2026-09-10, one extra blank line at end of file was removed from these four
retained Linux logs:

- `run02/01-probe-test.log`
- `run03/01-cli.log`
- `run03/02-workspace.log`
- `run03/04-doctest.log`

Only trailing newline characters after the final visible line were removed. Test
output, commands, results, and all other visible content are unchanged. Earlier
whitespace checks used narrower path scopes and did not inspect these retained
logs.
