use std::io::Write;
use std::path::PathBuf;

fn main() {
    // 界面用 app 的设计系统文件，拷一份进 ui/vendor（不进 git）
    let vendor = PathBuf::from("ui/vendor");
    let _ = std::fs::create_dir_all(&vendor);
    for f in ["tokens.css", "bundle.css", "illus.js"] {
        let src = PathBuf::from("../ui/vendor/moxian").join(f);
        println!("cargo:rerun-if-changed={}", src.display());
        std::fs::copy(&src, vendor.join(f)).expect("拷不了设计系统文件");
    }
    // 打包进去的 beayy.exe：BEAYY_SETUP_PAYLOAD 指向它（发布安装包.sh 先编 beayy.exe 再编这个）；没给就是空的，只能看界面
    println!("cargo:rerun-if-env-changed=BEAYY_SETUP_PAYLOAD");
    let data = match std::env::var("BEAYY_SETUP_PAYLOAD") {
        Ok(p) if !p.is_empty() => {
            println!("cargo:rerun-if-changed={p}");
            std::fs::read(&p).expect("读不了 BEAYY_SETUP_PAYLOAD")
        }
        _ => Vec::new(),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.deflate");
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&data).unwrap();
    std::fs::write(out, if data.is_empty() { Vec::new() } else { z.finish().unwrap() }).unwrap();
    // 名字里带 setup、清单里又没写要不要管理员的程序，Windows 会当成安装程序自动要管理员（弹 UAC）。
    // 这里只装给当前用户，写明 asInvoker：不弹，装出来的 Beayy 也不会以管理员身份跑
    println!("cargo:rerun-if-changed=manifest.xml");
    let attrs = tauri_build::Attributes::new().windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(include_str!("manifest.xml")));
    tauri_build::try_build(attrs).expect("tauri-build 失败")
}
