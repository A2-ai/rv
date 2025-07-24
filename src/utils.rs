use std::borrow::Cow;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::consts::NUM_CPUS_ENV_VAR_NAME;

pub(crate) fn get_max_workers() -> usize {
    std::env::var(NUM_CPUS_ENV_VAR_NAME)
        .ok()
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or_else(num_cpus::get)
}

pub(crate) fn create_spinner(visible: bool, message: impl Into<Cow<'static, str>>) -> ProgressBar {
    if visible {
        let pb = ProgressBar::new(10);
        pb.set_style(
            ProgressStyle::with_template("{spinner} {wide_msg}")
                .unwrap()
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(message);
        pb
    } else {
        ProgressBar::hidden()
    }
}

pub(crate) fn is_env_var_truthy(name: &str) -> bool {
    let val = std::env::var(name).unwrap_or_default().to_lowercase();

    val == "true" || val == "1"
}
