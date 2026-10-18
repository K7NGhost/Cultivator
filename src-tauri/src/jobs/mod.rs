mod manager;
pub(crate) use manager::{JobManager, RunControl, Workload};

pub(crate) fn cpu_parallelism() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .div_ceil(2)
        .clamp(1, 8)
}
