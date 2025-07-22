use anyhow::Result;
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

pub struct StepCoordinator {
    barrier: Arc<Barrier>,
}

impl StepCoordinator {
    pub fn new(thread_names: Vec<String>, _num_steps: usize) -> Self {
        let num_threads = thread_names.len();
        Self {
            barrier: Arc::new(Barrier::new(num_threads)),
        }
    }

    pub fn wait_for_step_start(
        &self,
        step_index: usize,
        thread_name: &str,
        _timeout: Option<Duration>,
    ) -> Result<()> {
        debug_print(&format!(
            "Thread {} hitting entry barrier for step {}",
            thread_name, step_index
        ));
        
        // Entry barrier - everyone ready for this step?
        self.barrier.wait();
        
        debug_print(&format!(
            "Thread {} proceeding with step {}",
            thread_name, step_index
        ));

        Ok(())
    }

    pub fn notify_step_completed(&self, step_index: usize, thread_name: &str) -> Result<()> {
        debug_print(&format!(
            "Thread {} hitting exit barrier for step {}",
            thread_name, step_index
        ));
        
        // Exit barrier - executing thread done, everyone can proceed
        self.barrier.wait();
        
        debug_print(&format!(
            "Thread {} proceeding past step {}",
            thread_name, step_index
        ));

        Ok(())
    }
}
