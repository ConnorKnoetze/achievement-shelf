//! Dragging an achievement out of the shelf as an image file, to drop into a chat app.
//!
//! Any file drag can be dropped on the desktop, which copies the file there. The shelf sits on
//! the desktop, so letting go just past it would leave a copy behind. This drag refuses the
//! desktop and the shelf itself: while the cursor is over them, the dragged data hides its file
//! formats, so they show the "no drop" cursor rather than offering to copy it, and letting go
//! there cancels the drag. Brought back over the shelf itself, the drag ends and the shelf
//! takes the slab back to be placed.
//!
//! The OS drag image is a still picture, so the slab is carried by a window of our own instead,
//! which keeps it swinging under the cursor anywhere on screen, as it does over the shelf.

use std::path::Path;

/// The image shown under the cursor while dragging: straight (not premultiplied) RGBA rows,
/// top first, held by the pixel at `hotspot`.
pub struct DragImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub hotspot: (i32, i32),
    /// The swing the slab had on the shelf, carried on from there; none for a still slab.
    pub swing: Option<Swing>,
    /// Device pixels per CSS pixel on the shelf, which the swing's speeds are measured in.
    pub pixel_ratio: f64,
}

/// The carried slab's swing (see `stepSwing` in shelf.ts): angle (deg), its rate (deg/s) and
/// the pointer's smoothed horizontal speed (CSS px/s).
#[derive(Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
pub struct Swing {
    pub angle: f64,
    pub rate: f64,
    pub speed: f64,
}

/// Whether the screen point (physical px) is over the shelf.
pub type OverShelf = Box<dyn Fn(i32, i32) -> bool + Send>;

/// How a drag out ended.
pub enum DragEnd {
    /// Dropped somewhere, or cancelled.
    Done,
    /// Brought back over the shelf with the button still down, at this screen point (physical
    /// px), swinging so; the shelf carries it on from there.
    Returned { x: i32, y: i32, swing: Swing },
}

#[cfg(windows)]
pub use windows_impl::drag_file;

#[cfg(not(windows))]
pub fn drag_file(_file: &Path, _image: DragImage, _over_shelf: OverShelf) -> Result<DragEnd, String> {
    Err("dragging achievements out is only supported on Windows".into())
}

#[cfg(windows)]
mod windows_impl {
    use super::{DragEnd, DragImage, OverShelf, Path, Swing};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Instant;
    use windows::core::{implement, w, Ref, BOOL, HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{
        COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, HWND, POINT, S_OK,
        SIZE,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, CLSCTX_INPROC_SERVER,
        FORMATETC, STGMEDIUM,
    };
    use windows::Win32::System::DataExchange::GetClipboardFormatNameW;
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, DROPEFFECT, DROPEFFECT_COPY};
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, CLSID_DragDropHelper, IDragSourceHelper, IShellItem, SHCreateItemFromParsingName,
        SHCreateStdEnumFmtEtc, SHDRAGIMAGE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetAncestor, GetClassNameW, GetCursorPos, GetWindowThreadProcessId, KillTimer,
        SetTimer, ShowWindow, UpdateLayeredWindow, WindowFromPoint, GA_ROOT, SW_SHOWNOACTIVATE, ULW_ALPHA,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    #[implement(IDropSource)]
    struct DropSource {
        over_shelf: OverShelf,
        /// Where the cursor came back over the shelf, ending the drag.
        returned: Rc<Cell<Option<POINT>>>,
    }

    impl IDropSource_Impl for DropSource_Impl {
        fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape_pressed.as_bool() {
                DRAGDROP_S_CANCEL
            } else if (key_state & MK_LBUTTON).0 != 0 {
                // Back over the shelf, the shelf takes the slab again to place it.
                let mut cursor = POINT::default();
                if unsafe { GetCursorPos(&mut cursor) }.is_ok() && (self.over_shelf)(cursor.x, cursor.y) {
                    self.returned.set(Some(cursor));
                    return DRAGDROP_S_CANCEL;
                }
                S_OK
            } else if over_refused_target() {
                DRAGDROP_S_CANCEL
            } else {
                DRAGDROP_S_DROP
            }
        }

        // Called as the mouse moves, so the carried slab keeps up with the cursor between ticks.
        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            update_follower();
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    /// Whether the window under the cursor is the desktop or the shelf, where a drop would
    /// only leave a stray copy of the image.
    fn over_refused_target() -> bool {
        unsafe {
            let mut point = POINT::default();
            if GetCursorPos(&mut point).is_err() {
                return true;
            }
            let window = WindowFromPoint(point);
            if window.is_invalid() {
                return true;
            }
            let root = GetAncestor(window, GA_ROOT);
            let mut pid = 0;
            GetWindowThreadProcessId(root, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                return true;
            }
            let mut class = [0u16; 16];
            let len = GetClassNameW(root, &mut class).max(0) as usize;
            // The desktop's icons sit in Progman, or in a WorkerW behind a wallpaper slideshow.
            matches!(String::from_utf16_lossy(&class[..len]).as_str(), "Progman" | "WorkerW")
        }
    }

    /// Formats that carry the file itself. The rest (such as the drag image the shell stores in
    /// the data) stay visible everywhere.
    fn is_file_format(format: *const FORMATETC) -> bool {
        let Some(format) = (unsafe { format.as_ref() }) else { return false };
        if format.cfFormat == CF_HDROP.0 {
            return true;
        }
        let mut name = [0u16; 64];
        let len = unsafe { GetClipboardFormatNameW(format.cfFormat as u32, &mut name) }.max(0) as usize;
        matches!(
            String::from_utf16_lossy(&name[..len]).as_str(),
            "Shell IDList Array"
                | "FileName"
                | "FileNameW"
                | "FileGroupDescriptor"
                | "FileGroupDescriptorW"
                | "FileContents"
                | "UniformResourceLocator"
                | "UniformResourceLocatorW"
        )
    }

    /// The shell's data for the file, with its file formats hidden from refused targets.
    /// Targets ask for formats as the cursor enters them, so each gets the answer for itself.
    #[implement(IDataObject)]
    struct GatedData(IDataObject);

    impl GatedData_Impl {
        fn hidden(&self, format: *const FORMATETC) -> bool {
            is_file_format(format) && over_refused_target()
        }
    }

    impl IDataObject_Impl for GatedData_Impl {
        fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            if self.hidden(format) {
                return Err(DV_E_FORMATETC.into());
            }
            unsafe { self.0.GetData(format) }
        }

        fn GetDataHere(&self, format: *const FORMATETC, medium: *mut STGMEDIUM) -> windows::core::Result<()> {
            if self.hidden(format) {
                return Err(DV_E_FORMATETC.into());
            }
            unsafe { self.0.GetDataHere(format, medium) }
        }

        fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
            if self.hidden(format) {
                return DV_E_FORMATETC;
            }
            unsafe { self.0.QueryGetData(format) }
        }

        fn GetCanonicalFormatEtc(&self, format: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
            unsafe { self.0.GetCanonicalFormatEtc(format, out) }
        }

        fn SetData(&self, format: *const FORMATETC, medium: *const STGMEDIUM, release: BOOL) -> windows::core::Result<()> {
            unsafe { self.0.SetData(format, medium, release.as_bool()) }
        }

        fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
            let all = unsafe { self.0.EnumFormatEtc(direction)? };
            if !over_refused_target() {
                return Ok(all);
            }
            let mut shown = Vec::new();
            let mut format = [FORMATETC::default()];
            while unsafe { all.Next(&mut format, None) } == S_OK {
                if !is_file_format(&format[0]) {
                    shown.push(format[0]);
                }
            }
            unsafe { SHCreateStdEnumFmtEtc(&shown) }
        }

        fn DAdvise(&self, format: *const FORMATETC, advf: u32, sink: Ref<IAdviseSink>) -> windows::core::Result<u32> {
            unsafe { self.0.DAdvise(format, advf, sink.as_ref()) }
        }

        fn DUnadvise(&self, connection: u32) -> windows::core::Result<()> {
            unsafe { self.0.DUnadvise(connection) }
        }

        fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
            unsafe { self.0.EnumDAdvise() }
        }
    }

    /// Straight RGBA to premultiplied BGRA, as GDI wants it.
    fn premultiplied_bgra(rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(rgba.len());
        for s in rgba.chunks_exact(4) {
            let a = s[3] as u16;
            let pre = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            out.extend_from_slice(&[pre(s[2]), pre(s[1]), pre(s[0]), s[3]]);
        }
        out
    }

    /// A top-down 32-bit DIB of `width` × `height`, and its pixels.
    fn dib(width: i32, height: i32) -> windows::core::Result<(HBITMAP, &'static mut [u8])> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down.
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        // Lives as long as the bitmap; callers drop the slice before deleting it.
        let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), width as usize * height as usize * 4) };
        Ok((bitmap, pixels))
    }

    /// A premultiplied bitmap of `image`, as the drag image helper wants it.
    fn bitmap(image: &DragImage) -> windows::core::Result<HBITMAP> {
        let (bitmap, pixels) = dib(image.width as i32, image.height as i32)?;
        pixels.copy_from_slice(&premultiplied_bgra(&image.rgba));
        Ok(bitmap)
    }

    /*
     * The carried slab hangs from the point it was grabbed by and swings as the cursor moves:
     * the same damped spring as `stepSwing` in shelf.ts, with the same constants.
     */
    const CARRY_TILT: f64 = -3.0;
    const SWING_STIFFNESS: f64 = 90.0;
    const SWING_DAMPING: f64 = 5.0;
    const SWING_PER_SPEED: f64 = 0.02;
    const MAX_SWING: f64 = 22.0;
    const SPEED_SMOOTHING: f64 = 0.05;
    /// Milliseconds between frames while the cursor is still; Windows rounds it to its tick.
    const FRAME_MS: u32 = 10;

    /// A click-through, topmost window holding the slab under the cursor. Being transparent to
    /// the mouse, it is never the drop target or the window `over_refused_target` finds.
    struct Follower {
        window: HWND,
        dc: HDC,
        canvas: HBITMAP,
        previous: HGDIOBJ,
        pixels: &'static mut [u8],
        /// The canvas is a square of this side, with the slab's hotspot at its centre.
        side: i32,
        /// The slab: premultiplied BGRA.
        slab: Vec<u8>,
        width: i32,
        height: i32,
        hotspot: (f64, f64),
        timer: usize,
        swing: Swing,
        pixel_ratio: f64,
        last_x: i32,
        last_time: Instant,
        /// The angle last drawn, to skip redrawing a slab at rest.
        drawn: Option<f64>,
    }

    thread_local! {
        static FOLLOWER: RefCell<Option<Follower>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
        update_follower();
    }

    fn update_follower() {
        FOLLOWER.with(|f| {
            if let Ok(mut f) = f.try_borrow_mut() {
                if let Some(f) = f.as_mut() {
                    f.update();
                }
            }
        });
    }

    impl Follower {
        fn new(image: &DragImage, swing: Swing) -> windows::core::Result<Self> {
            let (w, h) = (image.width as f64, image.height as f64);
            let (hx, hy) = (image.hotspot.0 as f64, image.hotspot.1 as f64);
            // Far enough from the hotspot for the slab at any angle.
            let reach = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
                .iter()
                .map(|(x, y)| (x - hx).hypot(y - hy))
                .fold(0.0, f64::max);
            let side = 2 * (reach.ceil() as i32 + 1);
            let mut cursor = POINT::default();
            unsafe {
                GetCursorPos(&mut cursor)?;
                let window = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("STATIC"),
                    PCWSTR::null(),
                    WS_POPUP,
                    cursor.x - side / 2,
                    cursor.y - side / 2,
                    side,
                    side,
                    None,
                    None,
                    None,
                    None,
                )?;
                let (canvas, pixels) = match dib(side, side) {
                    Ok(dib) => dib,
                    Err(e) => {
                        let _ = DestroyWindow(window);
                        return Err(e);
                    }
                };
                let dc = CreateCompatibleDC(None);
                let previous = SelectObject(dc, canvas.into());
                let mut follower = Follower {
                    window,
                    dc,
                    canvas,
                    previous,
                    pixels,
                    side,
                    slab: premultiplied_bgra(&image.rgba),
                    width: image.width as i32,
                    height: image.height as i32,
                    hotspot: (hx, hy),
                    timer: 0,
                    swing,
                    pixel_ratio: if image.pixel_ratio > 0.0 { image.pixel_ratio } else { 1.0 },
                    last_x: cursor.x,
                    last_time: Instant::now(),
                    drawn: None,
                };
                follower.update();
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                follower.timer = SetTimer(None, 0, FRAME_MS, Some(tick));
                Ok(follower)
            }
        }

        /// Advances the swing to now (as `stepSwing` does) and moves the slab to the cursor.
        fn update(&mut self) {
            let mut cursor = POINT::default();
            if unsafe { GetCursorPos(&mut cursor) }.is_err() {
                return;
            }
            let now = Instant::now();
            let dt = (now - self.last_time).as_secs_f64().min(1.0 / 30.0);
            if dt > 0.0 {
                let s = &mut self.swing;
                let raw = (cursor.x - self.last_x) as f64 / self.pixel_ratio / dt;
                s.speed += (raw - s.speed) * (1.0 - (-dt / SPEED_SMOOTHING).exp());
                let target = (s.speed * SWING_PER_SPEED).clamp(-MAX_SWING, MAX_SWING);
                s.rate += (SWING_STIFFNESS * (target - s.angle) - SWING_DAMPING * s.rate) * dt;
                s.angle += s.rate * dt;
                if s.angle.abs() < 0.05 && s.rate.abs() < 0.5 && s.speed.abs() < 1.0 {
                    *s = Swing::default();
                }
                self.last_x = cursor.x;
                self.last_time = now;
            }
            let angle = CARRY_TILT + self.swing.angle;
            if self.drawn.is_none_or(|drawn| (drawn - angle).abs() > 0.01) {
                self.draw(angle);
                self.drawn = Some(angle);
            }
            let position = POINT { x: cursor.x - self.side / 2, y: cursor.y - self.side / 2 };
            let size = SIZE { cx: self.side, cy: self.side };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            unsafe {
                let _ = UpdateLayeredWindow(
                    self.window,
                    None,
                    Some(&position),
                    Some(&size),
                    Some(self.dc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
            }
        }

        /// Draws the slab turned `angle` degrees clockwise about its hotspot, at the canvas centre.
        fn draw(&mut self, angle: f64) {
            self.pixels.fill(0);
            let (sin, cos) = angle.to_radians().sin_cos();
            let centre = (self.side / 2) as f64;
            let (hx, hy) = self.hotspot;
            // The canvas pixels the turned slab can cover.
            let (w, h) = (self.width as f64, self.height as f64);
            let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
                .map(|(x, y)| (x - hx, y - hy))
                .map(|(x, y)| (centre + x * cos - y * sin, centre + x * sin + y * cos));
            let span = |pick: fn(&(f64, f64)) -> f64| {
                let lo = corners.iter().map(pick).fold(f64::INFINITY, f64::min).floor().max(0.0) as i32;
                let hi = corners.iter().map(pick).fold(f64::NEG_INFINITY, f64::max).ceil().min(self.side as f64) as i32;
                lo..hi
            };
            let (xs, ys) = (span(|c| c.0), span(|c| c.1));
            let (w, h) = (self.width, self.height);
            let texel = |x: i32, y: i32| -> [f32; 4] {
                if x < 0 || y < 0 || x >= w || y >= h {
                    return [0.0; 4];
                }
                let i = (y * w + x) as usize * 4;
                let p = &self.slab[i..i + 4];
                [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
            };
            for py in ys {
                let dy = py as f64 + 0.5 - centre;
                let row = py as usize * self.side as usize * 4;
                for px in xs.clone() {
                    let dx = px as f64 + 0.5 - centre;
                    // Back from the canvas to the slab: turned the other way.
                    let u = hx + dx * cos + dy * sin - 0.5;
                    let v = hy - dx * sin + dy * cos - 0.5;
                    let (x0, y0) = (u.floor(), v.floor());
                    let (fx, fy) = ((u - x0) as f32, (v - y0) as f32);
                    let (x0, y0) = (x0 as i32, y0 as i32);
                    let (a, b, c, d) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
                    let out = &mut self.pixels[row + px as usize * 4..][..4];
                    for k in 0..4 {
                        let top = a[k] + (b[k] - a[k]) * fx;
                        let bottom = c[k] + (d[k] - c[k]) * fx;
                        out[k] = (top + (bottom - top) * fy + 0.5) as u8;
                    }
                }
            }
        }
    }

    impl Drop for Follower {
        fn drop(&mut self) {
            unsafe {
                if self.timer != 0 {
                    let _ = KillTimer(None, self.timer);
                }
                let _ = DestroyWindow(self.window);
                SelectObject(self.dc, self.previous);
                let _ = DeleteDC(self.dc);
                let _ = DeleteObject(self.canvas.into());
            }
        }
    }

    /// Drags `file` until the mouse button is released or the cursor comes back over the shelf.
    /// Must be called on the UI thread, with the button down.
    pub fn drag_file(file: &Path, image: DragImage, over_shelf: OverShelf) -> Result<DragEnd, String> {
        if image.rgba.len() != image.width as usize * image.height as usize * 4 {
            return Err("drag image size does not match its pixels".into());
        }
        unsafe {
            // Already done by the window's event loop; harmless again.
            let _ = OleInitialize(None);
            let item: IShellItem =
                SHCreateItemFromParsingName(&HSTRING::from(file.as_os_str()), None).map_err(|e| e.to_string())?;
            let shell_data: IDataObject = item.BindToHandler(None, &BHID_DataObject).map_err(|e| e.to_string())?;
            let data: IDataObject = GatedData(shell_data).into();

            // The follower carries the slab; the shell then gets a blank image, so targets that
            // draw the drag image themselves don't show a second, still slab. Without a
            // follower (with reduced motion, or if it cannot be made), the shell shows the slab.
            let follower = image.swing.and_then(|swing| {
                Follower::new(&image, swing)
                    .inspect_err(|e| eprintln!("achievement-shelf: cannot carry the slab swinging: {e}"))
                    .ok()
            });
            let shown = if follower.is_some() {
                DragImage { rgba: vec![0; 4], width: 1, height: 1, hotspot: (0, 0), swing: None, pixel_ratio: 1.0 }
            } else {
                image
            };

            let bitmap = bitmap(&shown).map_err(|e| e.to_string())?;
            let helper: Option<IDragSourceHelper> = CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok();
            let drag_image = SHDRAGIMAGE {
                sizeDragImage: SIZE { cx: shown.width as i32, cy: shown.height as i32 },
                ptOffset: POINT { x: shown.hotspot.0, y: shown.hotspot.1 },
                hbmpDragImage: bitmap,
                crColorKey: Default::default(),
            };
            // The helper owns the bitmap once it takes it.
            if helper.is_none_or(|h| h.InitializeFromBitmap(&drag_image, &data).is_err()) {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }

            let returned = Rc::new(Cell::new(None));
            let source: IDropSource = DropSource { over_shelf, returned: returned.clone() }.into();
            let mut effect = DROPEFFECT::default();
            FOLLOWER.with(|f| *f.borrow_mut() = follower);
            let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
            let follower = FOLLOWER.with(|f| f.borrow_mut().take());
            Ok(match returned.get() {
                Some(cursor) => DragEnd::Returned {
                    x: cursor.x,
                    y: cursor.y,
                    swing: follower.map(|f| f.swing).unwrap_or_default(),
                },
                None => DragEnd::Done,
            })
        }
    }
}
