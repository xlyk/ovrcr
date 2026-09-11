# Issue tracker: GitHub

Issues and specs live in GitHub Issues for `xlyk/ovrcr`.
Use `rtk proxy gh` for GitHub operations.

## Conventions

- Read an issue and its discussion with `gh issue view NUMBER --comments`.
- List issues with `gh issue list`, requesting JSON fields when useful.
- Before creating an issue, check for an existing matching issue.
- Create issues with `gh issue create --title TITLE --body-file FILE`.
- Use body files for multiline descriptions and comments.
- Apply or remove labels with `gh issue edit`.
- Comment with `gh issue comment`; close with `gh issue close`.
- Use the label vocabulary in `docs/agents/triage-labels.md`.
- Verify the repository from the Git remote; specify
  `--repo xlyk/ovrcr` when operating outside its checkout.

When a skill says “publish to the issue tracker,” create a GitHub issue.
When it says “fetch the relevant ticket,” read the issue and comments.

## Pull requests as a triage surface

PRs as a request surface: no.

Implementation PRs retain the repository's existing review, CI and
merge-authorization requirements.
