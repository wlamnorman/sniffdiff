use std::fmt::Write as _;
use std::time::{Duration, Instant};

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct TimingReport {
    total: Duration,
    phases: Vec<TimingPhase>,
}

#[derive(Debug, Clone)]
struct TimingPhase {
    name: &'static str,
    duration: Duration,
}

impl TimingReport {
    pub fn render_text(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "timing:");
        let _ = writeln!(output, "  total: {}", format_duration(self.total));
        for phase in &self.phases {
            let _ = writeln!(
                output,
                "  {}: {}",
                phase.name,
                format_duration(phase.duration)
            );
        }
        output
    }
}

pub(crate) struct TimingRecorder {
    started_at: Instant,
    phases: Vec<TimingPhase>,
}

impl TimingRecorder {
    pub(crate) fn start() -> Self {
        Self {
            started_at: Instant::now(),
            phases: Vec::new(),
        }
    }

    pub(crate) fn time_result<T>(
        &mut self,
        name: &'static str,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let started_at = Instant::now();
        let result = f();
        self.phases.push(TimingPhase {
            name,
            duration: started_at.elapsed(),
        });
        result
    }

    pub(crate) fn finish(self) -> TimingReport {
        TimingReport {
            total: self.started_at.elapsed(),
            phases: self.phases,
        }
    }
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_secs_f64() * 1000.0;
    if millis < 10.0 {
        format!("{millis:.2}ms")
    } else if millis < 1000.0 {
        format!("{millis:.0}ms")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}
