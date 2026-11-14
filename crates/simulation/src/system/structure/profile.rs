//! Per-thread stage timing for the coverage report. Compiled out of normal
//! builds: `time` and `count` are then plain pass-throughs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// One thin coupled attempt, including every stage below except Envelope.
    Coupled,
    /// One pass of the coupled gas/load/radius loop (count only).
    Iteration,
    /// One radial integration at a trial central pressure.
    Trial,
    /// One greenhouse surface-temperature search, including its partitions.
    Warming,
    /// One gas/condensate column partition.
    Partition,
    /// One self-gravitating envelope solve, including its trials.
    Envelope,
}

#[cfg(test)]
impl Stage {
    pub const ALL: [Self; 6] = [
        Self::Coupled,
        Self::Iteration,
        Self::Trial,
        Self::Warming,
        Self::Partition,
        Self::Envelope,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Coupled => "coupled",
            Self::Iteration => "iteration",
            Self::Trial => "trial",
            Self::Warming => "warming",
            Self::Partition => "partition",
            Self::Envelope => "envelope",
        }
    }
}

/// Calls and inclusive seconds per stage, indexed as `Stage::ALL`.
#[cfg(test)]
pub(crate) type Snapshot = [(u64, f64); 6];

#[cfg(test)]
thread_local! {
    static TOTALS: std::cell::Cell<Snapshot> = const { std::cell::Cell::new([(0, 0.0); 6]) };
}

/// Read and clear this thread's totals.
#[cfg(test)]
pub(crate) fn take() -> Snapshot {
    TOTALS.replace([(0, 0.0); 6])
}

#[cfg(test)]
fn add(stage: Stage, seconds: f64) {
    let mut totals = TOTALS.get();
    let entry = &mut totals[stage as usize];
    entry.0 += 1;
    entry.1 += seconds;
    TOTALS.set(totals);
}

#[cfg(test)]
pub(crate) fn time<T>(stage: Stage, work: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let result = work();
    add(stage, start.elapsed().as_secs_f64());
    result
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn time<T>(_: Stage, work: impl FnOnce() -> T) -> T {
    work()
}

pub(crate) fn count(stage: Stage) {
    #[cfg(test)]
    add(stage, 0.0);
    #[cfg(not(test))]
    let _ = stage;
}
