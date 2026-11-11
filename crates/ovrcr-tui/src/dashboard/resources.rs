use super::Dashboard;
use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// Raw aggregate CPU counters for the current platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuCounters {
    /// user, system, idle, and nice ticks; each counter wraps at 32 bits.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MacOs([u32; 4]),
    /// The first eight aggregate `/proc/stat` counters, in kernel order.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Linux([u64; 8]),
}

/// One validated whole-machine memory reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryCounters {
    used_bytes: u64,
    total_bytes: u64,
}

impl MemoryCounters {
    pub fn new(used_bytes: u64, total_bytes: u64) -> Option<Self> {
        (total_bytes > 0 && used_bytes <= total_bytes).then_some(Self {
            used_bytes,
            total_bytes,
        })
    }

    pub fn used_bytes(self) -> u64 {
        self.used_bytes
    }

    pub fn total_bytes(self) -> u64 {
        self.total_bytes
    }
}

/// Raw values from one machine-reading attempt. A missing value means that read failed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MachineReadings {
    pub cpu: Option<CpuCounters>,
    pub memory: Option<MemoryCounters>,
}

impl MachineReadings {
    pub fn new(cpu: Option<CpuCounters>, memory: Option<MemoryCounters>) -> Self {
        Self { cpu, memory }
    }
}

/// Source for one synchronous, whole-machine reading.
pub trait MachineResourceSource: Send {
    fn read(&mut self) -> MachineReadings;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reading<T> {
    Unknown,
    Available(T),
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub cpu_percent: Reading<u8>,
    pub memory: Reading<MemoryCounters>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            cpu_percent: Reading::Unknown,
            memory: Reading::Unknown,
        }
    }
}

pub(super) struct Sampler {
    source: Box<dyn MachineResourceSource>,
    previous_cpu: Option<CpuCounters>,
    last_sample: Option<Instant>,
    enabled: bool,
    snapshot: Snapshot,
}

impl Sampler {
    pub(super) fn system() -> Self {
        Self::with_boxed_source(system_source())
    }

    fn with_boxed_source(source: Box<dyn MachineResourceSource>) -> Self {
        Self {
            source,
            previous_cpu: None,
            last_sample: None,
            enabled: true,
            snapshot: Snapshot::default(),
        }
    }

    fn sample_if_due(&mut self, now: Instant) -> bool {
        if !self.enabled
            || self
                .last_sample
                .is_some_and(|last| now.saturating_duration_since(last) < SAMPLE_INTERVAL)
        {
            return false;
        }

        self.last_sample = Some(now);
        let readings = self.source.read();
        self.snapshot.cpu_percent = match readings.cpu {
            Some(current) => self
                .previous_cpu
                .replace(current)
                .and_then(|previous| cpu_percent(previous, current))
                .map_or(Reading::Unknown, Reading::Available),
            None => {
                self.previous_cpu = None;
                Reading::Unavailable
            }
        };
        self.snapshot.memory = readings
            .memory
            .map(Reading::Available)
            .unwrap_or(Reading::Unavailable);
        true
    }

    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.previous_cpu = None;
        self.last_sample = None;
        self.snapshot = Snapshot::default();
    }

    fn replace_source(&mut self, source: Box<dyn MachineResourceSource>) {
        *self = Self::with_boxed_source(source);
    }
}

impl Dashboard {
    /// Replaces the whole-machine source. The next sample starts with an unknown CPU baseline.
    pub fn replace_resource_source(&mut self, source: impl MachineResourceSource + 'static) {
        let enabled = self.settings.resources.sidebar;
        self.resources.replace_source(Box::new(source));
        self.resources.set_enabled(enabled);
    }

    /// Samples on the Dashboard's existing redraw tick, at most once every two seconds.
    /// Returns true when a source read occurred.
    pub fn sample_resources_if_due(&mut self, now: Instant) -> bool {
        self.resources.sample_if_due(now)
    }

    pub(super) fn resource_snapshot(&self) -> Snapshot {
        self.resources.snapshot
    }

    pub(super) fn set_resource_sampling_enabled(&mut self, enabled: bool) {
        self.resources.set_enabled(enabled);
    }
}

fn cpu_percent(previous: CpuCounters, current: CpuCounters) -> Option<u8> {
    let (deltas, idle_delta): (Vec<u64>, u128) = match (previous, current) {
        (CpuCounters::MacOs(previous), CpuCounters::MacOs(current)) => {
            let deltas = previous
                .into_iter()
                .zip(current)
                .map(|(previous, current)| u64::from(current.wrapping_sub(previous)))
                .collect::<Vec<_>>();
            let idle = u128::from(deltas[2]);
            (deltas, idle)
        }
        (CpuCounters::Linux(previous), CpuCounters::Linux(current)) => {
            let deltas = previous
                .into_iter()
                .zip(current)
                .map(|(previous, current)| current.checked_sub(previous))
                .collect::<Option<Vec<_>>>()?;
            let idle = u128::from(deltas[3]) + u128::from(deltas[4]);
            (deltas, idle)
        }
        _ => return None,
    };
    let total = deltas.into_iter().map(u128::from).sum::<u128>();
    if total == 0 || idle_delta > total {
        return None;
    }
    let busy = total - idle_delta;
    Some(((busy * 100) / total).min(100) as u8)
}

#[cfg(any(target_os = "macos", test))]
fn memory_from_macos(
    internal_pages: u64,
    purgeable_pages: u64,
    wired_pages: u64,
    compressor_pages: u64,
    page_size: u64,
    total_bytes: u64,
) -> Option<MemoryCounters> {
    let app_pages = internal_pages.checked_sub(purgeable_pages)?;
    let used_pages = app_pages
        .checked_add(wired_pages)?
        .checked_add(compressor_pages)?;
    MemoryCounters::new(used_pages.checked_mul(page_size)?, total_bytes)
}

#[cfg(any(target_os = "linux", test))]
fn memory_from_linux_kib(total_kib: u64, available_kib: u64) -> Option<MemoryCounters> {
    let used_kib = total_kib.checked_sub(available_kib)?;
    MemoryCounters::new(used_kib.checked_mul(1024)?, total_kib.checked_mul(1024)?)
}

fn system_source() -> Box<dyn MachineResourceSource> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacSource::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxSource)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(UnavailableSource)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
struct UnavailableSource;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl MachineResourceSource for UnavailableSource {
    fn read(&mut self) -> MachineReadings {
        MachineReadings::default()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{CpuCounters, MachineReadings, MachineResourceSource, memory_from_linux_kib};
    use std::fs;

    pub(super) struct LinuxSource;

    impl MachineResourceSource for LinuxSource {
        fn read(&mut self) -> MachineReadings {
            let cpu = fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|text| parse_cpu(&text));
            let memory = fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|text| parse_memory(&text));
            MachineReadings::new(cpu, memory)
        }
    }

    fn parse_cpu(text: &str) -> Option<CpuCounters> {
        let mut fields = text.lines().next()?.split_whitespace();
        if fields.next()? != "cpu" {
            return None;
        }
        let ticks = fields
            .take(8)
            .map(|field| field.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        let ticks: [u64; 8] = ticks.try_into().ok()?;
        Some(CpuCounters::Linux(ticks))
    }

    fn parse_memory(text: &str) -> Option<super::MemoryCounters> {
        let mut total_kib = None;
        let mut available_kib = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            if !matches!(key, "MemTotal" | "MemAvailable") {
                continue;
            }
            let mut fields = value.split_whitespace();
            let kib = fields.next()?.parse::<u64>().ok()?;
            if fields.next()? != "kB" {
                return None;
            }
            match key {
                "MemTotal" => total_kib = Some(kib),
                "MemAvailable" => available_kib = Some(kib),
                _ => {}
            }
        }
        memory_from_linux_kib(total_kib?, available_kib?)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{CpuCounters, MachineReadings, MachineResourceSource, memory_from_macos};
    use std::mem::{offset_of, size_of, size_of_val};
    use std::ptr;

    pub(super) struct MacSource {
        host: Option<libc::mach_port_t>,
    }

    impl MacSource {
        pub(super) fn new() -> Self {
            let host = mach_host_self();
            Self {
                host: (host != 0).then_some(host),
            }
        }

        fn cpu(&self, host: libc::mach_port_t) -> Option<CpuCounters> {
            let mut info = unsafe { std::mem::zeroed::<libc::host_cpu_load_info>() };
            let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
            let result = unsafe {
                libc::host_statistics(
                    host,
                    libc::HOST_CPU_LOAD_INFO,
                    (&mut info as *mut libc::host_cpu_load_info).cast(),
                    &mut count,
                )
            };
            if result != libc::KERN_SUCCESS || count < libc::HOST_CPU_LOAD_INFO_COUNT {
                return None;
            }
            Some(CpuCounters::MacOs([
                info.cpu_ticks[0],
                info.cpu_ticks[1],
                info.cpu_ticks[2],
                info.cpu_ticks[3],
            ]))
        }

        fn memory(&self, host: libc::mach_port_t) -> Option<super::MemoryCounters> {
            let mut info = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
            let mut count = libc::HOST_VM_INFO64_COUNT;
            let result = unsafe {
                libc::host_statistics64(
                    host,
                    libc::HOST_VM_INFO64,
                    (&mut info as *mut libc::vm_statistics64).cast(),
                    &mut count,
                )
            };
            let required_count = (offset_of!(libc::vm_statistics64, internal_page_count)
                + size_of::<libc::natural_t>())
            .div_ceil(size_of::<libc::integer_t>());
            if result != libc::KERN_SUCCESS || usize::try_from(count).unwrap_or(0) < required_count
            {
                return None;
            }

            let mut total_bytes = 0_u64;
            let mut size = size_of_val(&total_bytes);
            let result = unsafe {
                libc::sysctlbyname(
                    c"hw.memsize".as_ptr(),
                    (&mut total_bytes as *mut u64).cast(),
                    &mut size,
                    ptr::null_mut(),
                    0,
                )
            };
            if result != 0 || size != size_of_val(&total_bytes) {
                return None;
            }
            let page_size = unsafe { libc::vm_page_size } as u64;
            memory_from_macos(
                u64::from(info.internal_page_count),
                u64::from(info.purgeable_count),
                u64::from(info.wire_count),
                u64::from(info.compressor_page_count),
                page_size,
                total_bytes,
            )
        }
    }

    impl MachineResourceSource for MacSource {
        fn read(&mut self) -> MachineReadings {
            let Some(host) = self.host else {
                return MachineReadings::default();
            };
            MachineReadings::new(self.cpu(host), self.memory(host))
        }
    }

    impl Drop for MacSource {
        fn drop(&mut self) {
            let Some(host) = self.host.take() else {
                return;
            };
            #[allow(deprecated)]
            let task = unsafe { libc::mach_task_self() };
            unsafe {
                mach_port_deallocate(task, host);
            }
        }
    }

    #[allow(deprecated)]
    fn mach_host_self() -> libc::mach_port_t {
        unsafe { libc::mach_host_self() }
    }

    unsafe extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CpuCounters, MachineReadings, MachineResourceSource, MemoryCounters, Reading, Sampler,
        Snapshot, cpu_percent, memory_from_linux_kib, memory_from_macos,
    };
    use std::collections::VecDeque;
    use std::sync::{Arc, atomic::AtomicUsize};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct FixedSource {
        readings: VecDeque<MachineReadings>,
        calls: Option<Arc<AtomicUsize>>,
    }

    impl MachineResourceSource for FixedSource {
        fn read(&mut self) -> MachineReadings {
            if let Some(calls) = &self.calls {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            self.readings.pop_front().unwrap_or_default()
        }
    }

    fn linux_reading(user: u64, idle: u64, used: u64) -> MachineReadings {
        MachineReadings::new(
            Some(CpuCounters::Linux([user, 0, 0, idle, 0, 0, 0, 0])),
            MemoryCounters::new(used, 1000),
        )
    }

    #[test]
    fn cpu_percent_uses_the_difference_between_two_linux_samples() {
        let previous = CpuCounters::Linux([100, 0, 50, 50, 0, 0, 0, 0]);
        let current = CpuCounters::Linux([140, 0, 70, 90, 0, 0, 0, 0]);

        assert_eq!(cpu_percent(previous, current), Some(60));
    }

    #[test]
    fn macos_cpu_counter_wrap_is_measured_per_state() {
        let previous = CpuCounters::MacOs([u32::MAX - 5, 10, 100, 0]);
        let current = CpuCounters::MacOs([3, 15, 110, 0]);

        assert_eq!(cpu_percent(previous, current), Some(58));
    }

    #[test]
    fn unchanged_or_decreasing_linux_counters_do_not_produce_a_percentage() {
        let sample = CpuCounters::Linux([100, 0, 50, 50, 0, 0, 0, 0]);
        assert_eq!(cpu_percent(sample, sample), None);
        assert_eq!(
            cpu_percent(sample, CpuCounters::Linux([99, 0, 50, 50, 0, 0, 0, 0])),
            None
        );
    }

    #[test]
    fn macos_memory_excludes_purgeable_file_cache_and_uses_installed_memory() {
        assert_eq!(
            memory_from_macos(500, 100, 50, 25, 4096, 4 * 1024 * 1024),
            Some(MemoryCounters {
                used_bytes: 475 * 4096,
                total_bytes: 4 * 1024 * 1024,
            })
        );
        assert_eq!(memory_from_macos(10, 11, 0, 0, 4096, 4096), None);
    }

    #[test]
    fn linux_memory_uses_mem_available_in_kibibytes() {
        assert_eq!(
            memory_from_linux_kib(1000, 600),
            Some(MemoryCounters {
                used_bytes: 400 * 1024,
                total_bytes: 1000 * 1024,
            })
        );
        assert_eq!(memory_from_linux_kib(600, 1000), None);
    }

    #[test]
    fn cpu_is_unknown_until_advancing_samples_and_sampling_is_throttled() {
        let source = FixedSource {
            readings: VecDeque::from([
                linux_reading(10, 10, 200),
                linux_reading(10, 10, 300),
                linux_reading(20, 15, 400),
            ]),
            calls: None,
        };
        let mut sampler = Sampler::with_boxed_source(Box::new(source));
        let start = Instant::now();

        assert!(sampler.sample_if_due(start));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unknown);
        assert!(!sampler.sample_if_due(start + Duration::from_millis(1999)));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unknown);
        assert!(sampler.sample_if_due(start + Duration::from_secs(2)));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unknown);
        assert!(sampler.sample_if_due(start + Duration::from_secs(4)));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Available(66));
        assert_eq!(
            sampler.snapshot.memory,
            Reading::Available(MemoryCounters {
                used_bytes: 400,
                total_bytes: 1000,
            })
        );
    }

    #[test]
    fn cpu_failure_clears_the_baseline_before_recovery() {
        let failed = MachineReadings::new(None, None);
        let mut sampler = Sampler::with_boxed_source(Box::new(FixedSource {
            readings: VecDeque::from([
                linux_reading(100, 100, 200),
                failed,
                linux_reading(200, 200, 300),
                linux_reading(220, 210, 400),
            ]),
            calls: None,
        }));
        let start = Instant::now();

        sampler.sample_if_due(start);
        sampler.sample_if_due(start + Duration::from_secs(2));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unavailable);
        sampler.sample_if_due(start + Duration::from_secs(4));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unknown);
        sampler.sample_if_due(start + Duration::from_secs(6));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Available(66));
    }

    #[test]
    fn disabling_sampling_skips_reads_and_reenabling_starts_with_unknown_cpu() {
        let calls = Arc::new(AtomicUsize::new(0));
        let readings = VecDeque::from([linux_reading(100, 100, 200), linux_reading(120, 110, 300)]);
        let source = FixedSource {
            readings,
            calls: Some(Arc::clone(&calls)),
        };
        let mut sampler = Sampler::with_boxed_source(Box::new(source));
        let start = Instant::now();
        sampler.set_enabled(false);
        assert!(!sampler.sample_if_due(start));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        sampler.set_enabled(true);
        assert!(sampler.sample_if_due(start));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Unknown);
        assert!(sampler.sample_if_due(start + Duration::from_secs(2)));
        assert_eq!(sampler.snapshot.cpu_percent, Reading::Available(66));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn missing_memory_is_unavailable_without_retaining_the_previous_reading() {
        let mut sampler = Sampler::with_boxed_source(Box::new(FixedSource {
            readings: VecDeque::from([
                linux_reading(100, 100, 200),
                MachineReadings::new(Some(CpuCounters::Linux([120, 0, 0, 110, 0, 0, 0, 0])), None),
            ]),
            calls: None,
        }));
        let start = Instant::now();

        sampler.sample_if_due(start);
        assert_eq!(
            sampler.snapshot.memory,
            Reading::Available(MemoryCounters {
                used_bytes: 200,
                total_bytes: 1000,
            })
        );
        sampler.sample_if_due(start + Duration::from_secs(2));
        assert_eq!(sampler.snapshot.memory, Reading::Unavailable);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn real_linux_machine_reading_has_valid_memory_and_advancing_cpu() {
        let mut source = super::linux::LinuxSource;
        let first = source.read();
        let memory = first.memory.expect("/proc/meminfo reading");
        assert!(memory.total_bytes() > 0);
        assert!(memory.used_bytes() <= memory.total_bytes());
        let cpu = first.cpu.expect("/proc/stat reading");
        std::thread::sleep(Duration::from_millis(30));
        let second = source.read().cpu.expect("second /proc/stat reading");
        let percent = cpu_percent(cpu, second).expect("CPU ticks advanced");
        assert!((0..=100).contains(&percent));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn real_macos_machine_reading_has_valid_memory_and_advancing_cpu() {
        let mut source = super::macos::MacSource::new();
        let first = source.read();
        let memory = first.memory.expect("host VM and installed-memory reading");
        assert!(memory.total_bytes() > 0);
        assert!(memory.used_bytes() <= memory.total_bytes());
        let cpu = first.cpu.expect("host CPU reading");
        std::thread::sleep(Duration::from_millis(30));
        let second = source.read().cpu.expect("second host CPU reading");
        let percent = cpu_percent(cpu, second).expect("CPU ticks advanced");
        assert!((0..=100).contains(&percent));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_proc_parsers_read_aggregate_cpu_and_mem_available() {
        assert_eq!(
            super::linux::parse_cpu("cpu 1 2 3 4 5 6 7 8 9 10\ncpu0 1 2 3 4"),
            Some(CpuCounters::Linux([1, 2, 3, 4, 5, 6, 7, 8]))
        );
        assert_eq!(
            super::linux::parse_memory("MemTotal: 1000 kB\nMemAvailable: 600 kB\n"),
            MemoryCounters::new(400 * 1024, 1000 * 1024)
        );
        assert_eq!(super::linux::parse_cpu("bogus"), None);
        assert_eq!(super::linux::parse_memory("MemTotal: 2 kB\n"), None);
    }

    #[test]
    fn empty_snapshot_does_not_invent_a_first_cpu_sample() {
        assert_eq!(Snapshot::default().cpu_percent, Reading::Unknown);
        assert_eq!(Snapshot::default().memory, Reading::Unknown);
    }
}
