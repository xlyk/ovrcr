## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

Open New agent session with Branch, Agent harness, Model and optional Prompt as its four main controls, and start an agent in a registered checkout selected by its current branch label. Keep secondary options collapsed and preserve separate shell creation.

## Acceptance criteria

- [ ] Build picker membership from existing inventory/registry data joined to live hierarchy labels by project/workspace identity. Search registered workspaces only, exclude retained-only synthetic rows, and distinguish registered-but-unavailable checkouts.
- [ ] Group by project; preselect the current eligible feature checkout; label root/detached states. Missing/unavailable current targets require a deliberate choice rather than silent retargeting, and an empty instance exposes existing registration.
- [ ] Opening the form is independent of selected-target launch permission: n works for a registered project without a launchable current workspace. Start still checks root/default branch, registry identity, checkout existence and ownership.
- [ ] Follow the same checkout if its branch changes, refresh its label without a drift confirmation, and show an inline running-agent/shared-files notice without another confirmation.
- [ ] Use the shared proven harness/model/prompt capabilities; preserve named/cloud choices and honest unsupported states. Apply destination defaults only to untouched choices and preserve explicit dependent harness/model/prompt/name edits.
- [ ] Expose the four main controls and collapsed Advanced options, with predictable focus, keyboard/mouse input isolation and no picker-Enter form submission. The child-creation entry is added by the following slice, not shipped as a dead control here.
- [ ] n, dispatch, labels, availability and Actions/Menu agree; New terminal/custom command remains separately discoverable with its existing guards. Preserve ordinary program input and the return-to-Browse boundary.
- [ ] Production compiled Dashboard/Server tests cover membership, unavailable/root-drifted opening versus Start, project defaults, branch-label drift and actual target input. Native CUA verifies selection/focus and responsive 80×24/60-column/minimum/undersized states with screenshots/AX and owned cleanup.

## Blocked by

- https://github.com/xlyk/ovrcr/issues/392
