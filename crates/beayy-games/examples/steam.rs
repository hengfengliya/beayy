//! 对着本机的 Steam 跑一会儿：`cargo run -p beayy-games --example steam -- <数据目录> <秒数> [间隔秒]`
//! 不看「游戏」开关；每次看完打印 Steam 的样子，事件写进 <数据目录>/devices/…/events.jsonl
use beayy_core::EventLog;
use beayy_games::steam::{read_now, Steam};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(a.get(1).expect("数据目录"));
    let secs: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let every: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let log = Arc::new(EventLog::new(root.clone(), "win-steam-test"));
    let mut s = Steam::new(&root, log, Arc::new(AtomicBool::new(false)));
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        let now = read_now();
        println!("{} running={} user={:?} root={:?}", chrono::Local::now().format("%H:%M:%S"), now.running, now.active_user, now.root);
        s.step(&now, chrono::Utc::now());
        std::thread::sleep(Duration::from_secs(every));
    }
}
