## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

Select the single Create new branch entry, create a real branch and registered workspace in a focused child dialog, then return with that exact destination selected and the parent session draft intact. Reuse the same creation flow for standalone w.

## Acceptance criteria

- [ ] Exactly one Create new branch entry opens the child; only one dialog is visible/interactive. Project is preselected and changeable, Branch name starts empty with an example, and Base is inside collapsed Workspace options.
- [ ] Reuse local default-branch discovery without assuming main, automatic fetch/update or an invalid prefix. Loading/failure is explicit; Create requires resolved or explicitly valid Base. Preserve project setup and automatic-local-terminal policy without starting the requested agent.
- [ ] Keep parent/child draft and focus under the existing form owner. Accepted lifecycle Ok must not clear this child or let provisional/hierarchy auto-attachment select early; preserve unrelated lifecycle behavior.
- [ ] Keep the child visible while creating, show pending state and reject duplicate Submit/pending cancellation without claiming an abort. Validation/Git failures keep useful inputs; partial success and unknown outcomes reconcile before retry.
- [ ] Retain the submitted project/workspace ID and correlation token; only matching successful completion plus matching registered hierarchy/inventory selects it. Force both completion/hierarchy orders, including early events before acceptance, and never infer success from a row alone.
- [ ] Cancel before submission restores the old target/draft with no creation. Success restores harness/model/prompt/name and focuses the next unfinished control; later parent Cancel retains the committed workspace. Agent retry reuses it rather than recreating it.
- [ ] Standalone w uses the same creation semantics, selects the new workspace and returns to Browse. Preserve root/identity restrictions, one noncancellable lifecycle worker and live input in unrelated sessions.
- [ ] Production Git/socket/PTY and compiled Dashboard tests cover real mutations, duplicate branch/Base failure, event ordering, partial failure, retry and draft preservation. Native CUA covers both entry points, mouse/keyboard, narrow/error/pending states, actual target output and cleanup.

## Blocked by

- https://github.com/xlyk/ovrcr/issues/396
