pub fn write_err(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = format!("{err}");

    let mut cause = err.source();
    while let Some(e) = cause {
        out += &format!("\nReason: {e}");
        cause = e.source();
    }

    out
}

#[macro_export]
macro_rules! timeit {
    ($msg:expr, $x:expr) => {{
        let start = std::time::Instant::now();
        let res = $x;
        let duration = start.elapsed();
        log::info!("{} in {}ms", $msg, duration.as_millis());
        res
    }};
}

pub use timeit;
