//! 程序图标：时间线每段下面那排小图标（Windows 上 bundle_id 是程序完整路径）。
//! 走外壳的 IShellItemImageFactory，和资源管理器显示的是同一个图标；商店 app（WindowsApps 下的）也取得到。

use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};

/// 程序的图标，px×px 的 PNG；路径不在了、取不到返回 None
pub fn app_icon_png(path: &str, px: i32) -> Option<Vec<u8>> {
    if path.is_empty() || !std::path::Path::new(path).is_file() {
        return None;
    }
    let (w, h, mut buf) = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let hbm = f.GetImage(SIZE { cx: px, cy: px }, SIIGBF_ICONONLY).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut _));
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w.max(0) * h.max(0) * 4) as usize];
        let dc = GetDC(None);
        let n = GetDIBits(dc, hbm, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(hbm.into());
        if n == 0 || buf.is_empty() {
            return None;
        }
        (w, h, buf)
    };
    // BGRA → RGBA。没有透明通道的老图标当全不透明；预乘过的（颜色不超过透明度）还原回来
    let has_alpha = buf.chunks(4).any(|p| p[3] != 0);
    let premultiplied = has_alpha && buf.chunks(4).all(|p| p[0].max(p[1]).max(p[2]) <= p[3]);
    for p in buf.chunks_mut(4) {
        let a = if has_alpha { p[3] } else { 255 };
        let un = |c: u8| if !premultiplied { c } else if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        let (b, g, r) = (p[0], p[1], p[2]);
        p[0] = un(r);
        p[1] = un(g);
        p[2] = un(b);
        p[3] = a;
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w as u32, h as u32);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().ok()?.write_image_data(&buf).ok()?;
    }
    Some(out)
}
