//! Console frame presentation (queue slot 30): `(planeCount, planes, params)`, plane 0 holds the
//! frame's `ID3D12Resource**` at +0x10. Frames are copied into a DXGI flip swap chain bound to the
//! title's own top-level window and presented with vsync (which also paces the title).
#![allow(dead_code)]
use super::*;

type Vt = *const usize;
unsafe fn vt(o: *mut c_void) -> Vt {
    *(o as *const Vt)
}
unsafe fn m<T>(o: *mut c_void, slot: usize) -> T {
    std::mem::transmute_copy(&*vt(o).add(slot))
}

const IID_FACTORY2: [u8; 16] = [0x1c, 0x3a, 0xc8, 0x50, 0x72, 0xe0, 0x48, 0x4c, 0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0];
const IID_RESOURCE: [u8; 16] = [0xbe, 0x42, 0x64, 0x69, 0x2e, 0xa7, 0x59, 0x40, 0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad];
const IID_SWAP3: [u8; 16] = [0xdb, 0x9b, 0xd9, 0x94, 0xf8, 0xf1, 0xb0, 0x4a, 0xb2, 0x36, 0x7d, 0xa0, 0x17, 0x0e, 0xda, 0xb1];
const IID_DEVICE: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
const IID_ALLOC: [u8; 16] = [0xe4, 0xde, 0x02, 0x61, 0x59, 0xaf, 0x09, 0x4b, 0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24];
const IID_GFXLIST: [u8; 16] = [0x0f, 0x0d, 0x16, 0x5b, 0x1b, 0xac, 0x85, 0x41, 0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55];
const IID_QUEUE: [u8; 16] = [0xa6, 0x70, 0xc8, 0x0e, 0x7e, 0x5d, 0x22, 0x4c, 0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed]; // 0ec870a6-5d7e-4c22-8cfc-5baae07616ed;
const IID_FENCE: [u8; 16] = [0xcf, 0x3d, 0x75, 0x0a, 0xd8, 0xc4, 0x91, 0x4b, 0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76];

const RING: usize = 3;

struct Presenter {
    queue: usize,
    title_queue: usize,
    swap: usize, // IDXGISwapChain3
    buffers: Vec<usize>,
    allocs: [usize; RING],
    list: usize,
    fence: usize,
    /// title queue -> our queue ordering; separate from ence (two queues signalling one fence can move it backwards)
    tfence: usize,
    event: usize,
    fence_val: u64,
    slot_fence: [u64; RING],
    frame: usize,
    w: u32,
    h: u32,
    fmt: u32,
}
unsafe impl Send for Presenter {}

static PRESENTER: Mutex<Option<Presenter>> = Mutex::new(None);
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) static FRAMES: AtomicU64 = AtomicU64::new(0);

extern "system" {
    fn EnumWindows(cb: unsafe extern "system" fn(usize, isize) -> i32, l: isize) -> i32;
    fn GetWindowThreadProcessId(h: usize, pid: *mut u32) -> u32;
    fn IsWindowVisible(h: usize) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn GetClientRect(h: usize, r: *mut [i32; 4]) -> i32;
    fn LoadLibraryA(n: *const u8) -> usize;
    fn GetProcAddress(h: usize, n: *const u8) -> usize;
    fn CreateEventW(a: *mut c_void, manual: i32, init: i32, name: *const u16) -> usize;
    fn WaitForSingleObject(h: usize, ms: u32) -> u32;
}

/// The title's largest visible top-level window.
unsafe fn title_window() -> usize {
    unsafe extern "system" fn cb(h: usize, l: isize) -> i32 {
        let best = l as *mut (usize, i64);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(h) != 0 {
            let mut r = [0i32; 4];
            GetClientRect(h, &mut r);
            let area = (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64;
            if area > (*best).1 {
                *best = (h, area);
            }
        }
        1
    }
    let mut best: (usize, i64) = (0, 0);
    EnumWindows(cb, &mut best as *mut _ as isize);
    best.0
}

/// Swap chains accept only a few formats; map typeless/srgb variants to a presentable one.
fn presentable(fmt: u32) -> u32 {
    match fmt {
        27 | 28 | 29 => 28, // R8G8B8A8 typeless/unorm/srgb
        87 | 90 | 91 => 87, // B8G8R8A8
        23 | 24 => 24,      // R10G10B10A2
        9 | 10 => 10,       // R16G16B16A16_FLOAT
        _ => 0,
    }
}

unsafe fn create(queue: *mut c_void, src: *mut c_void) -> Result<Presenter, String> {
    let hwnd = title_window();
    if hwnd == 0 {
        return Err("no title window".into());
    }
    let mut d = [0u64; 7];
    let gd: unsafe extern "system" fn(*mut c_void, *mut [u64; 7]) -> *mut [u64; 7] = m(src, 10);
    gd(src, &mut d);
    let (w, h, sfmt) = (d[2] as u32, d[3] as u32, d[4] as u32);
    let fmt = presentable(sfmt);
    if fmt == 0 {
        return Err(format!("unpresentable format {sfmt}"));
    }
    let dxgi = LoadLibraryA(b"dxgi.dll\0".as_ptr());
    let cf2 = GetProcAddress(dxgi, b"CreateDXGIFactory2\0".as_ptr());
    if cf2 == 0 {
        return Err("CreateDXGIFactory2 missing".into());
    }
    let cf2: unsafe extern "system" fn(u32, *const u8, *mut *mut c_void) -> i32 = std::mem::transmute(cf2);
    let mut fac = std::ptr::null_mut();
    let hr = cf2(0, IID_FACTORY2.as_ptr(), &mut fac);
    if hr < 0 {
        return Err(format!("factory {hr:08x}"));
    }
    // DXGI_SWAP_CHAIN_DESC1: W, H, Format, Stereo, Sample{1,0}, Usage=RT_OUTPUT, Count, Scaling=STRETCH,
    // SwapEffect=FLIP_DISCARD, Alpha=UNSPECIFIED, Flags.
    let desc: [u32; 12] = [w, h, fmt, 0, 1, 0, 0x20, 2, 0, 4, 0, 0];
    // Present from our own DIRECT queue: the title's present queue may not be one DXGI accepts.
    let gdev: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = m(queue, 7);
    let mut dev = std::ptr::null_mut();
    gdev(queue, IID_DEVICE.as_ptr(), &mut dev);
    if dev.is_null() {
        return Err("queue GetDevice".into());
    }
    let cq: unsafe extern "system" fn(*mut c_void, *const u32, *const u8, *mut *mut c_void) -> i32 = m(dev, 8);
    let qdesc = [0u32; 4];
    let mut own = std::ptr::null_mut();
    let hr = cq(dev, qdesc.as_ptr(), IID_QUEUE.as_ptr(), &mut own);
    if hr < 0 {
        return Err(format!("CreateCommandQueue {hr:08x}"));
    }
    let title_queue = queue;
    let queue = own;
    let csc: unsafe extern "system" fn(*mut c_void, *mut c_void, usize, *const u32, *const c_void, *mut c_void, *mut *mut c_void) -> i32 = m(fac, 15);
    let mut sc1 = std::ptr::null_mut();
    let hr = csc(fac, queue, hwnd, desc.as_ptr(), std::ptr::null(), std::ptr::null_mut(), &mut sc1);
    if hr < 0 {
        return Err(format!("CreateSwapChainForHwnd {hr:08x} {w}x{h} fmt={fmt}"));
    }
    let qi: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = m(sc1, 0);
    let mut sc = std::ptr::null_mut();
    let hr = qi(sc1, IID_SWAP3.as_ptr(), &mut sc);
    if hr < 0 || sc.is_null() {
        return Err(format!("QI IDXGISwapChain3 {hr:08x} sc1={sc1:p}"));
    }
    let mut buffers = Vec::new();
    let gb: unsafe extern "system" fn(*mut c_void, u32, *const u8, *mut *mut c_void) -> i32 = m(sc, 9);
    for i in 0..2 {
        let mut b = std::ptr::null_mut();
        gb(sc, i, IID_RESOURCE.as_ptr(), &mut b);
        buffers.push(b as usize);
    }
    let ca: unsafe extern "system" fn(*mut c_void, u32, *const u8, *mut *mut c_void) -> i32 = m(dev, 9);
    let mut allocs = [0usize; RING];
    for a in allocs.iter_mut() {
        let mut p = std::ptr::null_mut();
        ca(dev, 0, IID_ALLOC.as_ptr(), &mut p);
        *a = p as usize;
    }
    let cl: unsafe extern "system" fn(*mut c_void, u32, u32, *mut c_void, *mut c_void, *const u8, *mut *mut c_void) -> i32 = m(dev, 12);
    let mut list = std::ptr::null_mut();
    let hr = cl(dev, 0, 0, allocs[0] as *mut c_void, std::ptr::null_mut(), IID_GFXLIST.as_ptr(), &mut list);
    if hr < 0 {
        return Err(format!("CreateCommandList {hr:08x}"));
    }
    let close: unsafe extern "system" fn(*mut c_void) -> i32 = m(list, 9);
    close(list);
    let cfn: unsafe extern "system" fn(*mut c_void, u64, u32, *const u8, *mut *mut c_void) -> i32 = m(dev, 36);
    let mut fence = std::ptr::null_mut();
    cfn(dev, 0, 0, IID_FENCE.as_ptr(), &mut fence);
        let mut tfence = std::ptr::null_mut();
        cfn(dev, 0, 0, IID_FENCE.as_ptr(), &mut tfence);
    log_d3d12(&format!("[present] swap chain {w}x{h} fmt={fmt} (src fmt {sfmt}) hwnd={hwnd:#x}"));
    Ok(Presenter {
        queue: queue as usize,
        title_queue: title_queue as usize,
        swap: sc as usize,
        buffers,
        allocs,
        list: list as usize,
        fence: fence as usize,
            tfence: tfence as usize,
        event: CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()),
        fence_val: 0,
        slot_fence: [0; RING],
        frame: 0,
        w,
        h,
        fmt: sfmt,
    })
}

/// D3D12_RESOURCE_BARRIER (transition, all subresources).
fn transition(res: usize, before: u32, after: u32) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[8..16].copy_from_slice(&res.to_le_bytes());
    b[16..20].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
    b[20..24].copy_from_slice(&before.to_le_bytes());
    b[24..28].copy_from_slice(&after.to_le_bytes());
    b
}

impl Presenter {
    unsafe fn present(&mut self, src: *mut c_void) {
        let slot = self.frame % RING;
        let fence = self.fence as *mut c_void;
        // Wait until this ring slot's previous copy retired.
        let done: unsafe extern "system" fn(*mut c_void) -> u64 = m(fence, 8);
        if done(fence) < self.slot_fence[slot] {
            let on: unsafe extern "system" fn(*mut c_void, u64, usize) -> i32 = m(fence, 9);
            on(fence, self.slot_fence[slot], self.event);
            WaitForSingleObject(self.event, 1000);
        }
        let alloc = self.allocs[slot] as *mut c_void;
        let list = self.list as *mut c_void;
        let ar: unsafe extern "system" fn(*mut c_void) -> i32 = m(alloc, 8);
        ar(alloc);
        let reset: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 = m(list, 10);
        reset(list, alloc, std::ptr::null_mut());
        let sc = self.swap as *mut c_void;
        let idx: unsafe extern "system" fn(*mut c_void) -> u32 = m(sc, 36);
        let bb = self.buffers[idx(sc) as usize % self.buffers.len()];
        const COPY_DEST: u32 = 0x400;
        const COPY_SRC: u32 = 0x800;
        let bar: unsafe extern "system" fn(*mut c_void, u32, *const [u8; 32]) = m(list, 26);
        let pre = [transition(src as usize, 0, COPY_SRC), transition(bb, 0, COPY_DEST)];
        bar(list, 2, pre.as_ptr());
        // CopyTextureRegion(dst{res, SUBRESOURCE_INDEX, 0}, 0,0,0, src{res, SUBRESOURCE_INDEX, 0}, box*)
        let mut dloc = [0u8; 40];
        dloc[..8].copy_from_slice(&bb.to_le_bytes());
        let mut sloc = [0u8; 40];
        sloc[..8].copy_from_slice(&(src as usize).to_le_bytes());
        let ctr: unsafe extern "system" fn(*mut c_void, *const u8, u32, u32, u32, *const u8, *const u32) = m(list, 16);
        let bx: [u32; 6] = [0, 0, 0, self.w, self.h, 1];
        ctr(list, dloc.as_ptr(), 0, 0, 0, sloc.as_ptr(), bx.as_ptr());
        let post = [transition(src as usize, COPY_SRC, 0), transition(bb, COPY_DEST, 0)];
        bar(list, 2, post.as_ptr());
        let close: unsafe extern "system" fn(*mut c_void) -> i32 = m(list, 9);
        let hr = close(list);
        let q = self.queue as *mut c_void;
        // Order the copy after the title's work: title queue signals, our queue waits.
        let tq = self.title_queue as *mut c_void;
        self.fence_val += 1;
        let tsig: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = std::mem::transmute(console_list::queue_orig(tq, 14));
        tsig(tq, self.tfence as *mut c_void, self.fence_val);
        let wait: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = m(q, 15);
        wait(q, self.tfence as *mut c_void, self.fence_val);
        // Debug flag percy_no_blit: present without copying the title's image (hang bisect).
        static NO_BLIT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let no_blit = *NO_BLIT.get_or_init(|| super::pso_guard::dir().join("percy_no_blit").exists());
        if hr >= 0 && !no_blit {
            let exec: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = m(q, 10);
            exec(q, 1, &list);
        }
        self.fence_val += 1;
        let signal: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = m(q, 14);
        signal(q, fence, self.fence_val);
        self.slot_fence[slot] = self.fence_val;
        let pres: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 = m(sc, 8);
        let phr = pres(sc, 1, 0);
        let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_power_of_two() || phr < 0 {
            log_d3d12(&format!("[present] frame {n} close={:08x} present={:08x}", hr as u32, phr as u32));
        }
        self.frame += 1;
    }
}

/// Queue slot 30 entry: present plane 0's resource.
pub(crate) unsafe fn present_x(q: *mut c_void, planes: u32, plane: *const u8) -> i32 {
    if planes == 0 || plane.is_null() || FAILED.load(Ordering::Relaxed) {
        return 0;
    }
    let pp = *(plane.add(0x10) as *const *const *mut c_void);
    if pp.is_null() || (*pp).is_null() {
        return 0;
    }
    let src = *pp;
    let mut g = PRESENTER.lock().unwrap();
    if g.is_none() {
        match create(q, src) {
            Ok(p) => *g = Some(p),
            Err(e) => {
                log_d3d12(&format!("[present] setup failed: {e}"));
                FAILED.store(true, Ordering::Relaxed);
                return 0;
            }
        }
    }
    g.as_mut().unwrap().present(src);
    0
}
