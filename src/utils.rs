use crate::consts::NUM_CPUS_ENV_VAR_NAME;

pub(crate) fn get_max_workers() -> usize {
    std::env::var(NUM_CPUS_ENV_VAR_NAME)
        .ok()
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or_else(num_cpus::get)
}
