# Linux attempt 02 — init corrected, environment failures retained

Same cad39ab product snapshot. Runner uses Docker `--init`; exact version-probe descendant-cleanup regression passed 1/1. Full workspace retry then reached CLI integration tests: 22 passed, 6 failed. Earlier binaries' exact counts remain in the raw log; later binaries did not run.

Five failures reported `SHELL is unset` during workspace creation. The remaining `runtime_errors_include_the_cause_chain` case expects EACCES through a chmod-000 directory; this container ran as root and instead reached ENOENT. These are runner differences from the intended unprivileged Linux host. No test or application behavior was changed. Post-run ps showed only docker-init, sleep and the inspection process; no retained test process was observed.

Next attempt sets SHELL=/bin/bash and runs as an unprivileged container user. CI also sets SHELL explicitly; the hosted Ubuntu runner is already unprivileged. Rerun the owning CLI binary before the workspace gate.
