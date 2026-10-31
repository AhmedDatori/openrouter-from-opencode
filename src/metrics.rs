//! In-process performance monitor: CPU, memory, concurrency, throughput and latency.
//!
//! A background thread samples every `INTERVAL` and keeps one hour of history in memory
//! for the dashboard. A summary line is written to stdout (data\server.log on Windows)
//! every minute while traffic is flowing, and every 15 minutes when idle.
use crate::App;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

pub const INTERVAL: Duration = Duration::from_secs(5);
const HISTORY: usize = 720;
const LOG_EVERY: usize = 12;
const IDLE_LOG_EVERY: usize = 180;
/// Latency values kept for the since-reset percentiles; older runs fall back to samples.
const SUMMARY_LATENCIES: usize = 200_000;

#[derive(Clone, Serialize)]
pub struct Sample {
    /// Unix time in milliseconds.
    pub t: i64,
    /// Gateway process CPU as a percentage of one core (100 = one full vCPU).
    pub cpu_core: f32,
    /// Gateway process CPU as a percentage of the whole machine.
    pub cpu: f32,
    /// Whole-machine CPU percentage, including other programs.
    pub sys_cpu: f32,
    /// Gateway resident memory (Windows working set) in bytes.
    pub rss: u64,
    pub sys_used: u64,
    pub sys_total: u64,
    /// API requests open at the sample instant (including streams still sending).
    pub active: usize,
    /// Highest number of open API requests at any moment during the interval.
    pub active_max: usize,
    /// Inference concurrency slots held at the sample instant.
    pub slots: usize,
    /// Database writes waiting for the SQLite worker.
    pub db_queue: usize,
    pub req: u64,
    pub rps: f64,
    pub err: u64,
    pub rej: u64,
    pub p50: Option<u32>,
    pub p95: Option<u32>,
    pub max: Option<u32>,
    pub ttft50: Option<u32>,
    pub ttft95: Option<u32>,
}

#[derive(Default)]
struct Window {
    durations: Vec<u32>,
    ttft: Vec<u32>,
}

struct Summary {
    since: i64,
    samples: u64,
    cpu_core_sum: f64,
    cpu_core_max: f32,
    rss_max: u64,
    rss_first: u64,
    sys_used_max: u64,
    active_max: usize,
    slots_max: usize,
    db_queue_max: usize,
    requests: u64,
    errors: u64,
    rejected: u64,
    durations: Vec<u32>,
    ttft: Vec<u32>,
}
impl Summary {
    fn new() -> Self {
        Self {
            since: chrono::Utc::now().timestamp_millis(),
            samples: 0,
            cpu_core_sum: 0.0,
            cpu_core_max: 0.0,
            rss_max: 0,
            rss_first: 0,
            sys_used_max: 0,
            active_max: 0,
            slots_max: 0,
            db_queue_max: 0,
            requests: 0,
            errors: 0,
            rejected: 0,
            durations: vec![],
            ttft: vec![],
        }
    }
    fn add(&mut self, s: &Sample, window: &Window) {
        if self.samples == 0 {
            self.rss_first = s.rss
        }
        self.samples += 1;
        self.cpu_core_sum += f64::from(s.cpu_core);
        self.cpu_core_max = self.cpu_core_max.max(s.cpu_core);
        self.rss_max = self.rss_max.max(s.rss);
        self.sys_used_max = self.sys_used_max.max(s.sys_used);
        self.active_max = self.active_max.max(s.active_max);
        self.slots_max = self.slots_max.max(s.slots);
        self.db_queue_max = self.db_queue_max.max(s.db_queue);
        self.requests += s.req;
        self.errors += s.err;
        self.rejected += s.rej;
        let room = SUMMARY_LATENCIES.saturating_sub(self.durations.len());
        self.durations
            .extend(window.durations.iter().take(room).copied());
        let room = SUMMARY_LATENCIES.saturating_sub(self.ttft.len());
        self.ttft.extend(window.ttft.iter().take(room).copied());
    }
    fn json(&self, now: i64) -> serde_json::Value {
        let seconds = ((now - self.since) as f64 / 1000.0).max(1.0);
        let mut d = self.durations.clone();
        let mut t = self.ttft.clone();
        d.sort_unstable();
        t.sort_unstable();
        serde_json::json!({
            "since": self.since,
            "seconds": seconds.round(),
            "samples": self.samples,
            "cpu_core_avg": if self.samples > 0 { self.cpu_core_sum / self.samples as f64 } else { 0.0 },
            "cpu_core_max": self.cpu_core_max,
            "rss_first": self.rss_first,
            "rss_max": self.rss_max,
            "sys_used_max": self.sys_used_max,
            "active_max": self.active_max,
            "slots_max": self.slots_max,
            "db_queue_max": self.db_queue_max,
            "requests": self.requests,
            "rps": self.requests as f64 / seconds,
            "errors": self.errors,
            "rejected": self.rejected,
            "p50": pct(&d, 50.0), "p95": pct(&d, 95.0), "p99": pct(&d, 99.0), "max": d.last(),
            "ttft50": pct(&t, 50.0), "ttft95": pct(&t, 95.0), "ttft99": pct(&t, 99.0),
        })
    }
}

pub struct Metrics {
    started: Instant,
    active: AtomicUsize,
    interval_peak: AtomicUsize,
    requests: AtomicU64,
    errors: AtomicU64,
    rejected: AtomicU64,
    window: Mutex<Window>,
    history: Mutex<VecDeque<Sample>>,
    summary: Mutex<Summary>,
    pub cpu_count: AtomicUsize,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            active: AtomicUsize::new(0),
            interval_peak: AtomicUsize::new(0),
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            window: Mutex::new(Window::default()),
            history: Mutex::new(VecDeque::with_capacity(HISTORY)),
            summary: Mutex::new(Summary::new()),
            cpu_count: AtomicUsize::new(1),
        }
    }
}

/// Counts an open API request until dropped, even if the client disconnects early.
pub struct Active(App);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.metrics.active.fetch_sub(1, Relaxed);
    }
}

impl Metrics {
    pub fn start(app: &App) -> Active {
        let now = app.metrics.active.fetch_add(1, Relaxed) + 1;
        app.metrics.interval_peak.fetch_max(now, Relaxed);
        Active(app.clone())
    }
    /// Records a finished API request. Latency percentiles only use inference calls.
    pub fn finish(&self, inference: bool, error: bool, duration_ms: u64, ttft_ms: Option<u64>) {
        self.requests.fetch_add(1, Relaxed);
        if error {
            self.errors.fetch_add(1, Relaxed);
        }
        if inference && !error {
            let mut w = self.window.lock().unwrap();
            w.durations.push(duration_ms.min(u32::MAX as u64) as u32);
            if let Some(t) = ttft_ms {
                w.ttft.push(t.min(u32::MAX as u64) as u32)
            }
        }
    }
    pub fn rejected(&self) {
        self.rejected.fetch_add(1, Relaxed);
    }
    pub fn reset(&self) {
        *self.summary.lock().unwrap() = Summary::new();
    }
    pub fn snapshot(&self, since: i64) -> serde_json::Value {
        let now = chrono::Utc::now().timestamp_millis();
        let history = self.history.lock().unwrap();
        let samples: Vec<&Sample> = history.iter().filter(|s| s.t > since).collect();
        serde_json::json!({
            "now": now,
            "interval_seconds": INTERVAL.as_secs(),
            "uptime_seconds": self.started.elapsed().as_secs(),
            "cpu_count": self.cpu_count.load(Relaxed),
            "active": self.active.load(Relaxed),
            "current": history.back(),
            "summary": self.summary.lock().unwrap().json(now),
            "samples": samples,
        })
    }
}

fn pct(sorted: &[u32], p: f64) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn log_line(samples: &[Sample], max_concurrent: usize) -> String {
    let n = samples.len().max(1) as f32;
    let cpu_avg = samples.iter().map(|s| s.cpu_core).sum::<f32>() / n;
    let cpu_max = samples.iter().map(|s| s.cpu_core).fold(0.0, f32::max);
    let last = samples.last().unwrap();
    let rss_max = samples.iter().map(|s| s.rss).max().unwrap_or(0);
    let req: u64 = samples.iter().map(|s| s.req).sum();
    let seconds = samples.len() as f64 * INTERVAL.as_secs_f64();
    let fmt = |v: Option<u32>| v.map_or("-".into(), |v| format!("{v}ms"));
    let p95 = samples.iter().filter_map(|s| s.p95).max();
    let ttft95 = samples.iter().filter_map(|s| s.ttft95).max();
    format!(
        "perf {} cpu_avg={:.1}% cpu_max={:.1}% (of one core) rss={:.1}MiB rss_max={:.1}MiB sys_mem={:.0}/{:.0}MiB sys_cpu={:.0}% active_max={} slots_max={}/{} db_queue_max={} req={} rps={:.2} err={} rejected={} p95_max={} ttft_p95_max={}",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        cpu_avg,
        cpu_max,
        mib(last.rss),
        mib(rss_max),
        mib(last.sys_used),
        mib(last.sys_total),
        last.sys_cpu,
        samples.iter().map(|s| s.active_max).max().unwrap_or(0),
        samples.iter().map(|s| s.slots).max().unwrap_or(0),
        max_concurrent,
        samples.iter().map(|s| s.db_queue).max().unwrap_or(0),
        req,
        req as f64 / seconds,
        samples.iter().map(|s| s.err).sum::<u64>(),
        samples.iter().map(|s| s.rej).sum::<u64>(),
        fmt(p95),
        fmt(ttft95),
    )
}

/// Starts the sampling thread. Sampling uses about 1 ms of CPU every 5 seconds.
pub fn spawn(app: App) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("metrics".into())
        .spawn(move || {
            let pid = sysinfo::get_current_pid().ok();
            let mut sys = System::new();
            let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
            let refresh = |sys: &mut System| {
                sys.refresh_cpu_usage();
                sys.refresh_memory();
                if let Some(pid) = pid {
                    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
                }
            };
            refresh(&mut sys);
            let cpus = sys.cpus().len().max(1);
            app.metrics.cpu_count.store(cpus, Relaxed);
            let mut last = Instant::now();
            let mut tick = 0usize;
            loop {
                std::thread::sleep(INTERVAL);
                refresh(&mut sys);
                let elapsed = last.elapsed().as_secs_f64().max(0.001);
                last = Instant::now();
                let m = &app.metrics;
                let mut window = std::mem::take(&mut *m.window.lock().unwrap());
                window.durations.sort_unstable();
                window.ttft.sort_unstable();
                let active = m.active.load(Relaxed);
                let (cpu_core, rss) = pid
                    .and_then(|pid| sys.process(pid))
                    .map_or((0.0, 0), |p| (p.cpu_usage(), p.memory()));
                let req = m.requests.swap(0, Relaxed);
                let sample = Sample {
                    t: chrono::Utc::now().timestamp_millis(),
                    cpu_core,
                    cpu: cpu_core / cpus as f32,
                    sys_cpu: sys.global_cpu_usage(),
                    rss,
                    sys_used: sys.used_memory(),
                    sys_total: sys.total_memory(),
                    active,
                    active_max: m.interval_peak.swap(active, Relaxed).max(active),
                    slots: app
                        .cfg
                        .max_concurrent
                        .saturating_sub(app.slots.available_permits()),
                    db_queue: app.db.queue_depth(),
                    req,
                    rps: req as f64 / elapsed,
                    err: m.errors.swap(0, Relaxed),
                    rej: m.rejected.swap(0, Relaxed),
                    p50: pct(&window.durations, 50.0),
                    p95: pct(&window.durations, 95.0),
                    max: window.durations.last().copied(),
                    ttft50: pct(&window.ttft, 50.0),
                    ttft95: pct(&window.ttft, 95.0),
                };
                m.summary.lock().unwrap().add(&sample, &window);
                let mut history = m.history.lock().unwrap();
                if history.len() == HISTORY {
                    history.pop_front();
                }
                history.push_back(sample);
                tick += 1;
                if tick.is_multiple_of(LOG_EVERY) {
                    let recent: Vec<Sample> = history
                        .iter()
                        .rev()
                        .take(LOG_EVERY)
                        .rev()
                        .cloned()
                        .collect();
                    drop(history);
                    let busy = recent.iter().any(|s| s.req > 0 || s.active > 0);
                    if busy || tick.is_multiple_of(IDLE_LOG_EVERY) {
                        println!("{}", log_line(&recent, app.cfg.max_concurrent));
                    }
                }
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::pct;
    #[test]
    fn percentiles_use_nearest_rank() {
        let v: Vec<u32> = (1..=100).collect();
        assert_eq!(pct(&v, 50.0), Some(50));
        assert_eq!(pct(&v, 95.0), Some(95));
        assert_eq!(pct(&v, 100.0), Some(100));
        assert_eq!(pct(&[7], 95.0), Some(7));
        assert_eq!(pct(&[], 50.0), None);
    }
}
