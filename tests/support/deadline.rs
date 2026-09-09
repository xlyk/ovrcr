//! One wait budget shared by the integration suites.

use std::time::Duration;

/// How long a test may wait for something it expects to happen.
///
/// Three seconds is generous on an idle developer machine and too tight on a
/// loaded CI runner, where the same poll loops lose whole scheduling quanta to
/// other jobs and the suites run in parallel with each other. Waits for an
/// expected event take this budget; bounds that assert something did *not*
/// happen keep their own, deliberately short, values.
pub fn wait_deadline() -> Duration {
    if std::env::var_os("CI").is_some() {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(3)
    }
}
