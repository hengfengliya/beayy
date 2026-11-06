//! 对着本机的客户端（或模拟的）跑一会儿：`BEAYY_LOL_LOCKFILE=… cargo run -p beayy-games --example watch -- <数据目录> <秒数>`
use beayy_core::EventLog;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(a.get(1).expect("数据目录"));
    let secs: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    let log = Arc::new(EventLog::new(root.clone(), "test-games"));
    let mut w = beayy_games::Watch::new(&root, log, Arc::new(AtomicBool::new(false)));
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        let wait = w.step();
        println!("connected={} wait={:?}", w.connected(), wait);
        std::thread::sleep(wait.min(Duration::from_secs(1)));
    }
}
