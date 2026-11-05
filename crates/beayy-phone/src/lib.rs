//! iPhone 屏幕使用：读 iPhone 经 iCloud 同步到这台 Mac 的 Biome 前台记录（App.InFocus 的 remote 流），
//! 写成 `app.focus` 事件，存 `devices/iphone-<同步目录 UUID 前 8 位>/`，按（时间、开始/结束、bundle id）去重，只增不改。
//! 需要「完全磁盘访问」。从 Swift 版 PhoneSync.swift 移植，解析规则同 脚本/日数据包.py 的 phone_events：
//! 每条记录里有 `10 01 18 <1 开始/0 结束> 21 <8 字节小端 double，Core Data 时间> 32 <长度> <bundle id>`。

use beayy_core::i18n::{tr, tr_with};
use beayy_core::{fields, iso, EventLog};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const CF_EPOCH: f64 = 978_307_200.0;
/// 最近这么多天有更新的同步目录才读（其余是不再用的旧设备）
const ACTIVE_DAYS: u64 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub t: f64,
    pub start: bool,
    pub bundle: String,
}

pub fn remote_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Biome/streams/restricted/App.InFocus/remote")
}

/// 能不能读 Biome（＝有没有完全磁盘访问）
pub fn can_read() -> bool {
    fs::read_dir(remote_root()).is_ok()
}

/// SEGB 文件里逐条找前台记录
pub fn parse(b: &[u8]) -> Vec<Record> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 15 < b.len() {
        if b[i] == 0x10 && b[i + 1] == 0x01 && b[i + 2] == 0x18 && b[i + 3] <= 1 && b[i + 4] == 0x21 && b[i + 13] == 0x32 {
            let n = b[i + 14] as usize;
            if i + 15 + n <= b.len() {
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&b[i + 5..i + 13]);
                let t = f64::from_le_bytes(raw) + CF_EPOCH;
                let bundle = String::from_utf8_lossy(&b[i + 15..i + 15 + n]).into_owned();
                out.push(Record { t, start: b[i + 3] == 1, bundle });
                i += 15 + n;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn key(t: f64, start: bool, bundle: &str) -> String {
    format!("{:.6}|{}|{}", t, if start { 1 } else { 0 }, bundle)
}

/// 读一台同步设备目录下的全部记录（按文件名顺序）
pub fn read_device_dir(dir: &Path) -> Vec<Record> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    files.sort();
    files.iter().filter_map(|f| fs::read(f).ok()).flat_map(|b| parse(&b)).collect()
}

fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()?.modified().ok()).max()
}

/// 已写过的记录：扫一遍这台设备已有的分段
fn load_seen(log: &EventLog) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(dirs) = fs::read_dir(log.segments_dir()) else { return out };
    for d in dirs.filter_map(|e| e.ok()) {
        let Ok(bytes) = fs::read(d.path().join("events.jsonl")) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if let (Some(t), Some(b), Some(s)) = (v["biome_time"].as_f64(), v["bundle_id"].as_str(), v["state"].as_str()) {
                out.insert(key(t, s == "start", b));
            }
        }
    }
    out
}

/// 同步器：记住每台 iPhone 的写入器和已写记录
#[derive(Default)]
pub struct PhoneSync {
    writers: HashMap<String, (EventLog, HashSet<String>)>,
    pub last_summary: String,
}

pub struct SyncNote {
    pub device: String,
    pub uuid: String,
    pub added: usize,
    pub total: usize,
    pub latest: String,
}

impl PhoneSync {
    /// 读一遍，把新记录写进 `root/devices/iphone-*/`；返回每台设备的情况
    pub fn sync(&mut self, root: &Path) -> Vec<SyncNote> {
        let mut notes = Vec::new();
        let Ok(dirs) = fs::read_dir(remote_root()) else {
            self.last_summary = tr("缺完全磁盘访问");
            return notes;
        };
        for dir in dirs.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
            let fresh = newest_mtime(&dir)
                .and_then(|m| SystemTime::now().duration_since(m).ok())
                .map(|age| age < Duration::from_secs(ACTIVE_DAYS * 86_400))
                .unwrap_or(false);
            if !fresh {
                continue;
            }
            let mut records = read_device_dir(&dir);
            // 只认 iPhone：有锁屏或主屏幕这类 iOS 才有的前台记录
            if !records.iter().any(|r| r.bundle == "com.apple.SleepLockScreen" || r.bundle == "com.apple.springboard") {
                continue;
            }
            records.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
            let uuid = dir.file_name().unwrap().to_string_lossy().to_string();
            let device = format!("iphone-{}", &uuid[..8.min(uuid.len())]);
            let entry = self.writers.entry(device.clone()).or_insert_with(|| {
                let log = EventLog::new(root.to_path_buf(), device.clone());
                let seen = load_seen(&log);
                (log, seen)
            });
            let (log, seen) = (&entry.0, &mut entry.1);
            let mut added = 0;
            for r in &records {
                let k = key(r.t, r.start, &r.bundle);
                if !seen.insert(k) {
                    continue;
                }
                let at = DateTime::<Utc>::from_timestamp_micros((r.t * 1e6) as i64);
                log.write(
                    "app.focus",
                    fields! {
                        "state" => if r.start { "start" } else { "end" },
                        "bundle_id" => r.bundle,
                        "biome_time" => r.t,
                        "source" => format!("biome:App.InFocus/remote/{uuid}"),
                        "recorded_at" => iso(Utc::now()),
                    },
                    at,
                );
                added += 1;
            }
            let latest = records
                .last()
                .and_then(|r| DateTime::<Utc>::from_timestamp_micros((r.t * 1e6) as i64))
                .map(iso)
                .unwrap_or_default();
            notes.push(SyncNote { device, uuid, added, total: records.len(), latest });
        }
        self.last_summary = if notes.is_empty() {
            tr("没有找到 iPhone 同步目录")
        } else {
            notes.iter().map(|n| tr_with("{device} 新增 {n} 条，同步截至 {latest}", &[("device", &n.device), ("n", &n.added.to_string()), ("latest", &n.latest)])).collect::<Vec<_>>().join(&tr("；"))
        };
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_record() {
        let mut b = vec![0u8, 0x10, 0x01, 0x18, 0x01, 0x21];
        b.extend_from_slice(&(800_000_000.5f64).to_le_bytes());
        b.push(0x32);
        let id = b"com.ss.iphone.ugc.Aweme";
        b.push(id.len() as u8);
        b.extend_from_slice(id);
        b.extend_from_slice(&[0, 0]);
        let r = parse(&b);
        assert_eq!(r, vec![Record { t: 800_000_000.5 + CF_EPOCH, start: true, bundle: "com.ss.iphone.ugc.Aweme".into() }]);
    }
}
