# Delegation, review, and delivery

- Honor the requested model, effort, and sequential/parallel workflow in the actual agent configuration. Give workers bounded briefs with current interfaces and explicit acceptance cases.
- Workers implement and self-review; the coordinator assigns independent review. Workers and reviewers must not recursively create duplicate review seats.
- Review the exact committed diff against the requirements and saved evidence. Check the real caller paths and test assertions, not just test names or passing counts.
- Return blocking findings to the same worker with a precise correction brief. Re-review the findings and correction diff; keep unrelated observations for the final feature review.
- Keep checkpoints current with completed work, actual commands/results, remaining cases, and blockers. Use the agent messaging tool when another agent needs a live update.
- Use `git` and `gh` for GitHub operations. Stage only intended files. Commit coherent reviewed units and honor existing authorization to push/open PRs; merging requires authorization too.
- PRs and handoffs state the problem, behavior change, exact verification, and remaining gaps. Keep roadmap checkboxes open until their required acceptance gates pass.
