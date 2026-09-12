# Review correction evidence

Base: 8ebbf0b with uncommitted correction. The first RED ran unchanged production from 8ebbf0b plus the new regression. Later checks include the correction; final committed-head gates belong to coordinator.
No compiler failures in this correction sequence.

Changes: commit the selected unread observation only after successful dashboard draw; both real event-loop draw sites use that boundary. The action uses the retained presented identity. No runtime or wire change.

- `review-red-presented-01.log`: `rtk proxy cargo test -p ovrcr-tui --lib unread_input_boundary -- --nocapture`; exit 101; 1 behavioral failure: queued key sent unseen B instead of last drawn A.
- `review-green-presented-01.log`: `rtk proxy cargo test -p ovrcr-tui --lib unread_input_boundary -- --nocapture`; exit 0; 1 passed after draw-committed target correction.
- `review-green-presented-02.log`: `rtk proxy cargo test -p ovrcr-tui --lib unread_review_tests -- --nocapture`; exit 0; 2 passed: actual event-loop message/input boundary and Unix socket writer; redraw advances target; failed draw, overlay, initial frame and changed selection stay safe.
- `review-tui-unread-green-01.log`: `rtk proxy cargo test -p ovrcr --test tui unread -- --nocapture`; exit 0; 3 passed.
- `review-lifetime-and-cli-green-01.log`: `rtk proxy cargo test -p ovrcr --test server_lifecycle codex_unread -- --nocapture`; exit 0; 2 passed: real CLI/dashboard review and removal followed by actual server shutdown/restart on same saved config.
- `review-notification-regression-green-01.log`: `rtk proxy cargo test -p ovrcr --test server_lifecycle desktop_notifications_queued_completion_is_cancelled_when_its_pane_becomes_visible -- --nocapture`; exit 0; 1 passed after literal row expectation updated for intentional unread dot.

Final cargo fmt --all -- --check and git diff --check exited 0.

SHA-256 of final corrected files before coordinator delivery:

- `crates/ovrcr-tui/src/dashboard/event_loop.rs`: `258d8fb4b437bd21d0c8fa68010758068cb56d6578723f80d5649de529c69642`
- `crates/ovrcr-tui/src/dashboard/mod.rs`: `322e8bb647c5f30c34684dc1663c2a0df3eb1d3c267c9f5ace81f53ab85d7f41`
- `crates/ovrcr-tui/src/dashboard/render.rs`: `86f90f2095324d65461e561824bc1bb2a4d040e61b1bc22b2455f712c575eefc`
- `crates/ovrcr-tui/src/dashboard/state.rs`: `355ca756864b2b97412b4a34f5fc4ab22776fbf8ab9de608118108747baf0d3d`
- `tests/server_lifecycle.rs`: `2da0272908285cc15d66c56fc5b9a054bf154ef5fb564679841a240351413151`
- `tests/tui/unread.rs`: `5ca42dccb030c1b846aca905a4d2cfd1c678cd4f9184b40638755456b7ebf6f7`
