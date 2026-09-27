//! Live resource readout for the settings tab: memory and CPU of this process.
//! Reads /proc on Linux; other platforms just say it is unavailable.

use std::time::Instant;

/// Kernel clock ticks per second. 100 on effectively every Linux build.
const CLK_TCK: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub rss_kb: u64,
    pub threads: u64,
    /// utime + stime, in clock ticks since the process started.
    pub cpu_ticks: u64,
}

/// Parse the two /proc files. Split out so it can be tested with canned text.
pub fn parse(status: &str, stat: &str) -> Option<Sample> {
    let field = |key: &str| -> Option<u64> {
        status.lines().find_map(|l| l.strip_prefix(key))?.split_whitespace().next()?.parse().ok()
    };
    let rss_kb = field("VmRSS:")?;
    let threads = field("Threads:")?;
    // The command name (field 2) may contain spaces and parens: skip past the last ')'.
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut f = rest.split_whitespace();
    // fields after ')' start at field 3 (state); utime is field 14, stime 15.
    let utime: u64 = f.nth(11)?.parse().ok()?;
    let stime: u64 = f.next()?.parse().ok()?;
    Some(Sample { rss_kb, threads, cpu_ticks: utime + stime })
}

pub fn sample() -> Option<Sample> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    parse(&status, &stat)
}

/// Turns successive samples into a CPU percentage.
pub struct Tracker {
    started: Instant,
    last: Option<(Instant, u64)>,
}

impl Tracker {
    pub fn new() -> Self {
        Self { started: Instant::now(), last: None }
    }

    /// Text block for the UI. CPU is measured since the previous call
    /// (first call: average since launch), 100% = one full core.
    pub fn report(&mut self, entries: usize) -> String {
        let Some(s) = sample() else {
            return "not available on this platform".into();
        };
        let now = Instant::now();
        let (since, ticks0) = self.last.unwrap_or((self.started, 0));
        let secs = now.duration_since(since).as_secs_f64().max(0.001);
        let cpu = (s.cpu_ticks.saturating_sub(ticks0)) as f64 / CLK_TCK / secs * 100.0;
        self.last = Some((now, s.cpu_ticks));
        let up = self.started.elapsed().as_secs();
        format!(
            "memory   {:.1} MB\ncpu      {:.1} %\nthreads  {}\nentries  {}\nuptime   {}m {:02}s",
            s.rss_kb as f64 / 1024.0,
            cpu,
            s.threads,
            entries,
            up / 60,
            up % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_text() {
        let status = "Name:\tpaddy\nVmRSS:\t   34567 kB\nThreads:\t7\n";
        // comm contains spaces and a paren on purpose
        let stat = "123 (pad) dy) S 1 2 3 4 5 6 7 8 9 10 250 50 0 0 20 0 7 0 100";
        let s = parse(status, stat).unwrap();
        assert_eq!(s, Sample { rss_kb: 34567, threads: 7, cpu_ticks: 300 });
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("", "").is_none());
        assert!(parse("VmRSS: 1 kB\nThreads: 1\n", "no parens").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_this_process() {
        let s = sample().expect("/proc is readable");
        assert!(s.rss_kb > 0 && s.threads >= 1);
        let text = Tracker::new().report(3);
        assert!(text.contains("memory") && text.contains("entries  3"));
    }
}
