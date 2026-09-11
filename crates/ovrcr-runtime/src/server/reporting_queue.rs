use std::sync::mpsc::{self, Receiver, SendError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReportingQueueSnapshot {
    pub pending_items: usize,
    pub pending_bytes: usize,
    pub peak_items: usize,
    pub peak_bytes: usize,
    pub rejected: u64,
}

#[derive(Default)]
struct QueueCounters {
    snapshot: Mutex<ReportingQueueSnapshot>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub struct ReportingQueueMonitor {
    counters: Arc<QueueCounters>,
}

impl ReportingQueueMonitor {
    #[cfg(feature = "acceptance-diagnostics")]
    pub fn snapshot(&self) -> ReportingQueueSnapshot {
        *self.counters.snapshot.lock().unwrap()
    }
}

pub struct ReportingSender<T> {
    sender: Option<SyncSender<T>>,
    counters: Arc<QueueCounters>,
    weight: fn(&T) -> usize,
    tracked: bool,
    #[cfg(test)]
    after_send: TestHook,
}

pub struct ReportingReceiver<T> {
    receiver: Option<Receiver<T>>,
    counters: Arc<QueueCounters>,
    weight: fn(&T) -> usize,
    tracked: bool,
    #[cfg(test)]
    before_wait: TestHook,
}

#[cfg(test)]
type TestHook = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

impl<T> Clone for ReportingSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: Some(self.sender.as_ref().unwrap().clone()),
            counters: Arc::clone(&self.counters),
            weight: self.weight,
            tracked: self.tracked,
            #[cfg(test)]
            after_send: Arc::clone(&self.after_send),
        }
    }
}

impl<T> ReportingSender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        if !self.tracked {
            return self.sender.as_ref().unwrap().send(value);
        }

        let bytes = (self.weight)(&value);
        let mut value = value;
        loop {
            let mut snapshot = self.counters.snapshot.lock().unwrap();
            match self.sender.as_ref().unwrap().try_send(value) {
                Ok(()) => {
                    self.run_test_hook();
                    record_enqueue(&mut snapshot, bytes);
                    self.counters.changed.notify_all();
                    return Ok(());
                }
                Err(TrySendError::Full(returned)) => {
                    value = returned;
                    drop(self.counters.changed.wait(snapshot).unwrap());
                }
                Err(TrySendError::Disconnected(returned)) => {
                    snapshot.rejected += 1;
                    return Err(SendError(returned));
                }
            }
        }
    }

    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        if !self.tracked {
            return self.sender.as_ref().unwrap().try_send(value);
        }

        let bytes = (self.weight)(&value);
        let mut snapshot = self.counters.snapshot.lock().unwrap();
        match self.sender.as_ref().unwrap().try_send(value) {
            Ok(()) => {
                self.run_test_hook();
                record_enqueue(&mut snapshot, bytes);
                self.counters.changed.notify_all();
                Ok(())
            }
            Err(error) => {
                snapshot.rejected += 1;
                Err(error)
            }
        }
    }

    pub fn snapshot(&self) -> ReportingQueueSnapshot {
        *self.counters.snapshot.lock().unwrap()
    }

    #[cfg(test)]
    fn run_test_hook(&self) {
        let hook = self.after_send.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    #[cfg(not(test))]
    fn run_test_hook(&self) {}
}

impl<T> Drop for ReportingSender<T> {
    fn drop(&mut self) {
        if self.tracked {
            let _snapshot = self.counters.snapshot.lock().unwrap();
            drop(self.sender.take());
            self.counters.changed.notify_all();
        }
    }
}

impl<T> ReportingReceiver<T> {
    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        if !self.tracked {
            return self.receiver.as_ref().unwrap().recv();
        }

        loop {
            let mut snapshot = self.counters.snapshot.lock().unwrap();
            match self.receiver.as_ref().unwrap().try_recv() {
                Ok(value) => {
                    record_dequeue(&mut snapshot, (self.weight)(&value));
                    self.counters.changed.notify_all();
                    return Ok(value);
                }
                Err(TryRecvError::Empty) => {
                    self.run_test_hook();
                    drop(self.counters.changed.wait(snapshot).unwrap());
                }
                Err(TryRecvError::Disconnected) => return Err(mpsc::RecvError),
            }
        }
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError> {
        if !self.tracked {
            return self.receiver.as_ref().unwrap().recv_timeout(timeout);
        }

        let deadline = Instant::now() + timeout;
        loop {
            let mut snapshot = self.counters.snapshot.lock().unwrap();
            match self.receiver.as_ref().unwrap().try_recv() {
                Ok(value) => {
                    record_dequeue(&mut snapshot, (self.weight)(&value));
                    self.counters.changed.notify_all();
                    return Ok(value);
                }
                Err(TryRecvError::Disconnected) => {
                    return Err(mpsc::RecvTimeoutError::Disconnected);
                }
                Err(TryRecvError::Empty) => {}
            }

            self.run_test_hook();

            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(mpsc::RecvTimeoutError::Timeout);
            };
            let (returned, wait) = self
                .counters
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap();
            snapshot = returned;
            if wait.timed_out() {
                match self.receiver.as_ref().unwrap().try_recv() {
                    Ok(value) => {
                        record_dequeue(&mut snapshot, (self.weight)(&value));
                        self.counters.changed.notify_all();
                        return Ok(value);
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Err(mpsc::RecvTimeoutError::Disconnected);
                    }
                    Err(TryRecvError::Empty) => return Err(mpsc::RecvTimeoutError::Timeout),
                }
            }
        }
    }

    #[cfg(test)]
    fn run_test_hook(&self) {
        let hook = self.before_wait.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    #[cfg(not(test))]
    fn run_test_hook(&self) {}
}

impl<T> Drop for ReportingReceiver<T> {
    fn drop(&mut self) {
        if !self.tracked {
            return;
        }
        let receiver = self.receiver.take().unwrap();
        let mut snapshot = self.counters.snapshot.lock().unwrap();
        while let Ok(value) = receiver.try_recv() {
            record_dequeue(&mut snapshot, (self.weight)(&value));
        }
        drop(receiver);
        self.counters.changed.notify_all();
    }
}

fn record_enqueue(snapshot: &mut ReportingQueueSnapshot, bytes: usize) {
    snapshot.pending_items += 1;
    snapshot.pending_bytes += bytes;
    snapshot.peak_items = snapshot.peak_items.max(snapshot.pending_items);
    snapshot.peak_bytes = snapshot.peak_bytes.max(snapshot.pending_bytes);
}

fn record_dequeue(snapshot: &mut ReportingQueueSnapshot, bytes: usize) {
    snapshot.pending_items = snapshot
        .pending_items
        .checked_sub(1)
        .expect("reporting queue item accounting underflow");
    snapshot.pending_bytes = snapshot
        .pending_bytes
        .checked_sub(bytes)
        .expect("reporting queue byte accounting underflow");
}

fn channel<T>(
    capacity: usize,
    weight: fn(&T) -> usize,
    counters: Arc<QueueCounters>,
    tracked: bool,
) -> (ReportingSender<T>, ReportingReceiver<T>) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        ReportingSender {
            sender: Some(sender),
            counters: Arc::clone(&counters),
            weight,
            tracked,
            #[cfg(test)]
            after_send: Arc::new(Mutex::new(None)),
        },
        ReportingReceiver {
            receiver: Some(receiver),
            counters,
            weight,
            tracked,
            #[cfg(test)]
            before_wait: Arc::new(Mutex::new(None)),
        },
    )
}

#[cfg(test)]
pub fn reporting_channel<T>(
    capacity: usize,
    weight: fn(&T) -> usize,
) -> (ReportingSender<T>, ReportingReceiver<T>) {
    channel(capacity, weight, Arc::new(QueueCounters::default()), true)
}

pub fn reporting_channel_with_monitor<T>(
    capacity: usize,
    weight: fn(&T) -> usize,
    monitor: &ReportingQueueMonitor,
) -> (ReportingSender<T>, ReportingReceiver<T>) {
    channel(capacity, weight, Arc::clone(&monitor.counters), true)
}

impl<T> From<SyncSender<T>> for ReportingSender<T> {
    fn from(sender: SyncSender<T>) -> Self {
        Self {
            sender: Some(sender),
            counters: Arc::new(QueueCounters::default()),
            weight: |_| 0,
            tracked: false,
            #[cfg(test)]
            after_send: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T> From<Receiver<T>> for ReportingReceiver<T> {
    fn from(receiver: Receiver<T>) -> Self {
        Self {
            receiver: Some(receiver),
            counters: Arc::new(QueueCounters::default()),
            weight: |_| 0,
            tracked: false,
            #[cfg(test)]
            before_wait: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn accounting_is_atomic_with_channel_refill() {
        let (sender, receiver) = reporting_channel(1, |value: &usize| *value);
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        *sender.after_send.lock().unwrap() = Some(Arc::new({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move || {
                entered.wait();
                release.wait();
            }
        }));

        let first = {
            let sender = sender.clone();
            thread::spawn(move || sender.try_send(10).unwrap())
        };
        entered.wait();
        let consume = thread::spawn(move || (receiver.recv().unwrap(), receiver));
        let refill = {
            let sender = sender.clone();
            thread::spawn(move || sender.send(20).unwrap())
        };
        release.wait();
        first.join().unwrap();
        let (consumed, receiver) = consume.join().unwrap();
        assert_eq!(consumed, 10);
        refill.join().unwrap();

        assert_eq!(sender.snapshot().pending_items, 1);
        assert_eq!(sender.snapshot().pending_bytes, 20);
        assert_eq!(sender.snapshot().peak_items, 1);
        assert_eq!(receiver.recv().unwrap(), 20);
        assert_eq!(sender.snapshot().pending_items, 0);
        assert_eq!(sender.snapshot().pending_bytes, 0);
    }

    #[test]
    fn blocked_send_wakes_when_receiver_drops() {
        let (sender, receiver) = reporting_channel(1, |_| 1);
        sender.send(1).unwrap();
        let blocked = thread::spawn(move || sender.send(2));
        drop(receiver);
        assert_eq!(blocked.join().unwrap(), Err(SendError(2)));
    }

    #[test]
    fn receiver_wakes_on_send_and_sender_drop() {
        let (sender, receiver) = reporting_channel(1, |_| 1);
        let receive = thread::spawn(move || {
            assert_eq!(receiver.recv().unwrap(), 1);
            assert_eq!(receiver.recv(), Err(mpsc::RecvError));
        });
        sender.send(1).unwrap();
        drop(sender);
        receive.join().unwrap();
    }

    #[test]
    fn sender_drop_cannot_be_lost_between_empty_and_wait() {
        let (sender, receiver) = reporting_channel::<usize>(1, |_| 1);
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        *receiver.before_wait.lock().unwrap() = Some(Arc::new({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move || {
                entered.wait();
                release.wait();
            }
        }));

        let receive = thread::spawn(move || receiver.recv());
        entered.wait();
        let drop_sender = thread::spawn(move || drop(sender));
        release.wait();
        drop_sender.join().unwrap();
        assert_eq!(receive.join().unwrap(), Err(mpsc::RecvError));
    }

    #[test]
    fn timeout_uses_channel_state_as_authority() {
        let (_sender, receiver) = reporting_channel::<usize>(1, |_| 1);
        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn receiver_drop_drains_accounting() {
        let (sender, receiver) = reporting_channel(2, |value: &usize| *value);
        sender.send(10).unwrap();
        sender.send(20).unwrap();
        drop(receiver);
        let snapshot = sender.snapshot();
        assert_eq!(snapshot.pending_items, 0);
        assert_eq!(snapshot.pending_bytes, 0);
        assert_eq!(sender.try_send(30), Err(TrySendError::Disconnected(30)));
        assert_eq!(sender.snapshot().rejected, 1);
    }
}
