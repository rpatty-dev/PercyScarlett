//! Replay of D3D12.X inline command records onto the PC command list.
//!
//! Console titles append records at the list write cursor instead of calling methods.
//! Before every method call (and at Close) the pending records are decoded and issued on
//! the PC list, so their order relative to method calls is preserved. Record encodings:
//!   0x4004C200 inst, startIndex, indexCount, baseVertex, startInstance  DrawIndexedInstanced
//!   0x4002C400 x, y, z                                                  Dispatch
//!   0x40nnC6ss (va | stride << 48) x (n+1)/2                            IASetVertexBuffers(ss)
//!   0x4001B7ff va (u64)                                                 IASetIndexBuffer(fmt ff)
//!   0xC000B600 topology / 0xC000B800 stencil ref
//!   root parameter: header from list +0x140 (graphics) / +0x340 (compute) + 4*i, written by
//!   our SetRootSignature; the inline code adds constant offset * 4 and (count - 1) << 16.
//!   pipeline state: 28 bytes copied from PSO +0x10 (our wrapper puts PSO_MARK + pointer there).
use super::console_list::{buf_start, inner_of, rewind, OBJ_SIZE, OFF_WP};
use super::*;
use std::collections::{BTreeMap, HashMap};

const T_FLUSH: usize = OBJ_SIZE - 0x60;
const T_GRS: usize = OBJ_SIZE - 0x68;
const T_CRS: usize = OBJ_SIZE - 0x70;
const T_VHEAP: usize = OBJ_SIZE - 0x78;
const T_SHEAP: usize = OBJ_SIZE - 0x80;
const GFX_HDRS: usize = 0x140;
const CS_HDRS: usize = 0x340;
const MAX_PARAMS: usize = 64;
const PARAM: u32 = 0xC000_0000;
const CS_BIT: u32 = 0x2000_0000;
pub(crate) const PSO_MARK: u32 = 0x8005_0002;

#[derive(Clone, Copy, Default)]
struct Param {
    kind: u8, // 0 table, 1 constants, 2 CBV, 3 SRV, 4 UAV
    sampler: bool,
}

static RS: Mutex<Option<HashMap<usize, Vec<Param>>>> = Mutex::new(None);
static BUFS: Mutex<BTreeMap<u64, (u64, u8)>> = Mutex::new(BTreeMap::new());
/// VB/IB binds per backing kind: 0 DEFAULT, 1 UMA (sysmem), 2 UPLOAD, 3 placed, 4 untracked.
pub(crate) static BIND_KIND: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
static STATS: [AtomicU32; 12] = [const { AtomicU32::new(0) }; 12];

unsafe fn at<T: Copy>(obj: *mut c_void, off: usize) -> T {
    std::ptr::read_unaligned((obj as *const u8).add(off) as *const T)
}
unsafe fn put<T>(obj: *mut c_void, off: usize, v: T) {
    std::ptr::write_unaligned((obj as *mut u8).add(off) as *mut T, v)
}
unsafe fn method(inner: *mut c_void, slot: usize) -> usize {
    *(*(inner as *const *const usize)).add(slot)
}

// ---- root signatures ----------------------------------------------------------------------

fn parse_rs(b: &[u8]) -> Option<Vec<Param>> {
    let rd = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let mut base = 0usize;
    if b.starts_with(b"DXBC") {
        let parts = rd(28)? as usize;
        base = (0..parts)
            .filter_map(|i| rd(32 + 4 * i))
            .map(|o| o as usize)
            .find(|&o| b.get(o..o + 4) == Some(b"RTS0"))?
            + 8;
    }
    let r = |o: usize| rd(base + o);
    let n = r(4)? as usize;
    let po = r(8)? as usize;
    let mut v = Vec::with_capacity(n);
    for i in 0..n.min(MAX_PARAMS) {
        let p = po + 12 * i;
        let kind = r(p)? as u8;
        let pay = r(p + 8)? as usize;
        let mut sampler = false;
        if kind == 0 && r(pay)? > 0 {
            sampler = r(r(pay + 4)? as usize)? == 3; // first range type SAMPLER
        }
        v.push(Param { kind, sampler });
    }
    Some(v)
}

pub(crate) unsafe fn register_rs(rs: *mut c_void, blob: *const u8, len: usize) {
    if rs.is_null() || blob.is_null() {
        return;
    }
    if let Some(p) = parse_rs(std::slice::from_raw_parts(blob, len)) {
        RS.lock().unwrap().get_or_insert_with(HashMap::new).insert(rs as usize, p);
    }
}

fn rs_params(rs: usize) -> Vec<Param> {
    RS.lock().unwrap().as_ref().and_then(|m| m.get(&rs).cloned()).unwrap_or_default()
}

/// SetGraphics/ComputeRootSignature: remember it and publish the parameter headers the
/// inline code copies into its records.
pub(crate) unsafe fn set_root_signature(obj: *mut c_void, rs: *mut c_void, cs: bool) {
    put(obj, if cs { T_CRS } else { T_GRS }, rs as usize);
    let base = if cs { CS_HDRS } else { GFX_HDRS };
    for (i, p) in rs_params(rs as usize).iter().enumerate() {
        let cnt = if p.kind >= 2 { 1u32 } else { 0 };
        let h = PARAM | if cs { CS_BIT } else { 0 } | (cnt << 16) | ((i as u32) << 10);
        put(obj, base + 4 * i, h);
    }
}

pub(crate) unsafe fn set_heaps(obj: *mut c_void, n: u32, heaps: *const *mut c_void) {
    for i in 0..n as usize {
        let h = *heaps.add(i);
        if h.is_null() {
            continue;
        }
        let gd: unsafe extern "system" fn(*mut c_void, *mut [u32; 4]) -> *mut [u32; 4] = std::mem::transmute(method(h, 8));
        let mut d = [0u32; 4];
        gd(h, &mut d);
        let gs: unsafe extern "system" fn(*mut c_void, *mut u64) -> *mut u64 = std::mem::transmute(method(h, 10));
        let mut start = 0u64;
        gs(h, &mut start);
        put(obj, if d[0] == 1 { T_SHEAP } else { T_VHEAP }, start);
    }
}

/// New recording: nothing pending, no root signatures bound.
pub(crate) unsafe fn reset_state(obj: *mut c_void) {
    put(obj, T_FLUSH, 0usize);
    put(obj, T_GRS, 0usize);
    put(obj, T_CRS, 0usize);
}

// ---- buffer extents (vertex / index views need a size) --------------------------------------

pub(crate) unsafe fn note_buffer(res: *mut c_void, width: u64, kind: u8) {
    if res.is_null() || width == 0 {
        return;
    }
    let va: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(method(res, 11));
    let a = va(res);
    if a != 0 {
        BUFS.lock().unwrap().insert(a, (a + width, kind));
    }
}

/// Bytes from `va` to the end of the PC buffer holding it (no bind statistics).
pub(crate) fn buf_extent(va: u64) -> Option<u64> {
    let m = BUFS.lock().unwrap();
    m.range(..=va).next_back().filter(|(_, &(end, _))| end > va).map(|(_, &(end, _))| end - va)
}
fn buf_bytes(va: u64) -> u32 {
    let m = BUFS.lock().unwrap();
    match m.range(..=va).next_back() {
        Some((_, &(end, k))) if end > va => {
            BIND_KIND[k as usize].fetch_add(1, Ordering::Relaxed);
            (end - va).min(u32::MAX as u64) as u32
        }
        _ => {
            BIND_KIND[4].fetch_add(1, Ordering::Relaxed);
            64 << 20
        }
    }
}

// ---- pipeline state wrapper ---------------------------------------------------------------

#[repr(C)]
struct PsoX {
    vtbl: usize,
    inner: *mut c_void,
    pkt: [u32; 7], // +0x10: copied into the stream by inline SetPipelineState
    pad: u32,
}

macro_rules! pfwd {
    ($n:ident, $s:literal) => {
        #[unsafe(naked)]
        unsafe extern "system" fn $n() {
            std::arch::naked_asm!("mov rcx, qword ptr [rcx + 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const $s * 8);
        }
    };
}
pfwd!(p1, 1);
pfwd!(p2, 2);
pfwd!(p3, 3);
pfwd!(p4, 4);
pfwd!(p5, 5);
pfwd!(p6, 6);
pfwd!(p7, 7);
pfwd!(p8, 8);
unsafe extern "system" fn p_qi(this: *mut PsoX, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let inner = (*this).inner;
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(method(inner, 0));
    let hr = f(inner, riid, out);
    if hr >= 0 && !out.is_null() && *out == inner {
        *out = this as *mut c_void;
    }
    hr
}
unsafe extern "system" fn p_stub(_: *mut PsoX) -> usize {
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 2 {
        log_d3d12("[rpl] PSO console-only method called");
    }
    0
}
static PSO_VTBL: OnceLock<usize> = OnceLock::new();
fn pso_vtbl() -> usize {
    *PSO_VTBL.get_or_init(|| {
        let mut v = [p_stub as usize; 16];
        v[0] = p_qi as usize;
        for (i, f) in [p1 as usize, p2 as usize, p3 as usize, p4 as usize, p5 as usize, p6 as usize, p7 as usize, p8 as usize].into_iter().enumerate() {
            v[i + 1] = f;
        }
        Box::leak(Box::new(v)).as_ptr() as usize
    })
}

pub(crate) unsafe fn wrap_pso(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return p;
    }
    // Replayed records outlive the title's Release (replay is deferred): keep the PC PSO alive.
    // shortcut: PSOs are never freed (the wrapper already leaks); upgrade with deferred release on fence.
    let add: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(method(p, 1));
    add(p);
    let mut pkt = [0u32; 7];
    pkt[0] = PSO_MARK;
    pkt[1] = p as usize as u32;
    pkt[2] = ((p as usize) >> 32) as u32;
    Box::leak(Box::new(PsoX { vtbl: pso_vtbl(), inner: p, pkt, pad: 0 })) as *mut PsoX as *mut c_void
}

pub(crate) unsafe fn unwrap_pso(p: *mut c_void) -> *mut c_void {
    if !p.is_null() && *(p as *const usize) == pso_vtbl() {
        (*(p as *const PsoX)).inner
    } else {
        p
    }
}

// ---- flush / replay ------------------------------------------------------------------------

/// Issue all records written since the last flush on the PC list, then rewind the cursor.
pub(crate) unsafe fn flush(obj: *mut c_void) {
    if (obj as usize) & 0xfff == 0xff8 {
        return; // guard-page probe object
    }
    let Some(inner) = inner_of(obj) else { return };
    let start = buf_start(obj);
    let wp: *mut u8 = at(obj, OFF_WP);
    let mut p: *mut u8 = at(obj, T_FLUSH);
    if p.is_null() || p < start || p > wp {
        p = start;
    }
    let n = (wp as usize).saturating_sub(p as usize) / 4;
    if n > 0 {
        replay(obj, inner, std::slice::from_raw_parts(p as *const u32, n));
    }
    rewind(obj);
    put(obj, T_FLUSH, start);
}

unsafe fn bad(obj: *mut c_void, w: &[u32], i: usize) {
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 8 {
        let ctx: Vec<String> = w[i.saturating_sub(4)..(i + 8).min(w.len())].iter().map(|x| format!("{x:x}")).collect();
        log_d3d12(&format!("[rpl] unknown record {:08x} at {i}/{} obj={obj:p}: {}", w[i], w.len(), ctx.join(" ")));
    }
}

unsafe fn replay(obj: *mut c_void, inner: *mut c_void, w: &[u32]) {
    type F2 = unsafe extern "system" fn(*mut c_void, u32);
    let mut ph = false; // bound PSO is the placeholder
    let mut i = 0;
    while i < w.len() {
        let h = w[i];
        let len = if h >> 30 == 2 && h != PSO_MARK { 1 } else { ((h >> 16) & 0xff) as usize + 2 };
        if i + len > w.len() {
            bad(obj, w, i);
            break;
        }
        let a = &w[i + 1..i + len];
        let q = |k: usize| a[k] as u64 | (a[k + 1] as u64) << 32;
        {
            static TR: AtomicU32 = AtomicU32::new(0);
            if TR.fetch_add(1, Ordering::Relaxed) < 300 {
                let ar: Vec<String> = a.iter().take(6).map(|x| format!("{x:x}")).collect();
                log_d3d12(&format!("[rpl] {h:08x} [{}] obj={obj:p}", ar.join(" ")));
            }
        }
        match h {
            0x4004_C200 => {
                // shortcut: no translated graphics PSOs yet; the bound PSO is the compute placeholder and a
                // draw with it removes the device on NVIDIA. Count only, until graphics translation lands.
                if std::env::var_os("PERCY_REPLAY_DRAWS").is_some() {
                    let f: unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) = std::mem::transmute(method(inner, 13));
                    f(inner, a[2], a[0], a[1], a[3] as i32, a[4]);
                }
                STATS[0].fetch_add(1, Ordering::Relaxed);
            }
            0x4002_C400 => {
                let f: unsafe extern "system" fn(*mut c_void, u32, u32, u32) = std::mem::transmute(method(inner, 14));
                if !ph { f(inner, a[0], a[1], a[2]); }
                STATS[1].fetch_add(1, Ordering::Relaxed);
            }
            0xC000_B600 => {
                let f: F2 = std::mem::transmute(method(inner, 20));
                f(inner, a[0]);
            }
            0xC000_B800 => {
                let f: F2 = std::mem::transmute(method(inner, 24));
                f(inner, a[0]);
            }
            PSO_MARK => {
                let pso = q(0) as usize;
                {
                    static PN: AtomicU32 = AtomicU32::new(0);
                    if PN.fetch_add(1, Ordering::Relaxed) < 12 {
                        let vt = if pso > 0x10000 { *(pso as *const usize) } else { 0 };
                        let gt: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(method(inner, 8));
                        let rc = if pso > 0x10000 {
                            let ad: unsafe extern "system" fn(usize) -> u32 = std::mem::transmute(method(pso as *mut c_void, 1));
                            let rl: unsafe extern "system" fn(usize) -> u32 = std::mem::transmute(method(pso as *mut c_void, 2));
                            ad(pso);
                            rl(pso)
                        } else { 0 };
                        const IID_DEV: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
                        let dev_of = |o: *mut c_void| {
                            let gd: unsafe extern "system" fn(*mut c_void, *const u8, *mut usize) -> i32 = std::mem::transmute(method(o, 7));
                            let mut d = 0usize;
                            let hr = gd(o, IID_DEV.as_ptr(), &mut d);
                            if hr >= 0 && d != 0 {
                                let rl: unsafe extern "system" fn(usize) -> u32 = std::mem::transmute(method(d as *mut c_void, 2));
                                rl(d);
                            }
                            (hr as u32, d)
                        };
                        let pd = if pso > 0x10000 { dev_of(pso as *mut c_void) } else { (0, 0) };
                        log_d3d12(&format!("[rpl] SetPipelineState pso={pso:#x} vt={vt:#x} rc={rc} list_type={} inner={inner:p} fb={:#x} dev pso={:x?} list={:x?}", gt(inner), super::pso_fallback_raw(), pd, dev_of(inner)));
                    }
                }
                let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(method(inner, 25));
                ph = pso == unwrap_pso(super::pso_fallback_raw() as *mut c_void) as usize;
                // shortcut: placeholder work is meaningless; binding it faulted NVIDIA (null+8). Skip until shaders translate.
                if !ph { f(inner, pso as *mut c_void); }
                STATS[2].fetch_add(1, Ordering::Relaxed);
            }
            _ if h & 0xFF00_FF00 == 0x4000_C600 => {
                let views: Vec<[u32; 4]> = a
                    .chunks_exact(2)
                    .map(|c| {
                        let v = c[0] as u64 | (c[1] as u64) << 32;
                        let va = v & 0xFFFF_FFFF_FFFF;
                        let size = if va == 0 { 0 } else { buf_bytes(va) };
                        [va as u32, (va >> 32) as u32, size, (v >> 48) as u32]
                    })
                    .collect();
                let f: unsafe extern "system" fn(*mut c_void, u32, u32, *const [u32; 4]) = std::mem::transmute(method(inner, 44));
                f(inner, h & 0xFF, views.len() as u32, views.as_ptr());
                STATS[3].fetch_add(1, Ordering::Relaxed);
            }
            _ if h & 0xFFFF_FF00 == 0x4001_B700 => {
                let va = q(0);
                let view = [va as u32, (va >> 32) as u32, if va == 0 { 0 } else { buf_bytes(va) }, h & 0xFF];
                let f: unsafe extern "system" fn(*mut c_void, *const [u32; 4]) = std::mem::transmute(method(inner, 43));
                f(inner, if va == 0 { std::ptr::null() } else { &view });
                STATS[4].fetch_add(1, Ordering::Relaxed);
            }
            _ if h >> 30 == 3 => {
                let cs = h & CS_BIT != 0;
                let gfx = !cs as usize;
                let idx = ((h >> 10) & 0x3F) as usize;
                let off = (h >> 2) & 0xFF;
                let rs: usize = at(obj, if cs { T_CRS } else { T_GRS });
                let Some(p) = rs_params(rs).get(idx).copied() else {
                    bad(obj, w, i);
                    break;
                };
                match p.kind {
                    0 => {
                        let start: u64 = at(obj, if p.sampler { T_SHEAP } else { T_VHEAP });
                        let mut hnd = (start & !0xFFFF_FFFF) | a[0] as u64;
                        if hnd < start {
                            hnd += 1 << 32;
                        }
                        let f: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(method(inner, 31 + gfx));
                        f(inner, idx as u32, hnd);
                    }
                    1 => {
                        let f: unsafe extern "system" fn(*mut c_void, u32, u32, *const u32, u32) = std::mem::transmute(method(inner, 35 + gfx));
                        f(inner, idx as u32, a.len() as u32, a.as_ptr(), off);
                    }
                    k @ 2..=4 => {
                        let f: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(method(inner, 37 + 2 * (k as usize - 2) + gfx));
                        f(inner, idx as u32, q(0));
                    }
                    _ => {}
                }
                STATS[5].fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                bad(obj, w, i);
                break;
            }
        }
        i += len;
    }
}

/// draws/dispatches/psos/vbs/ibs/params replayed so far.
pub(crate) fn stats() -> String {
    let s: Vec<u32> = STATS[..6].iter().map(|x| x.load(Ordering::Relaxed)).collect();
    format!("draw={} disp={} pso={} vb={} ib={} param={}", s[0], s[1], s[2], s[3], s[4], s[5])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rts0_params() {
        // bare RTS0 v1: 2 params (table of 1 SAMPLER range, CBV)
        let mut b = vec![0u8; 0];
        let w = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        w(&mut b, 1); // version
        w(&mut b, 2); // params
        w(&mut b, 24); // params offset
        w(&mut b, 0);
        w(&mut b, 0);
        w(&mut b, 0);
        // params @24: {type, vis, payload}
        w(&mut b, 0); w(&mut b, 0); w(&mut b, 48);
        w(&mut b, 2); w(&mut b, 0); w(&mut b, 56);
        // table payload @48: {nranges, off}
        w(&mut b, 1); w(&mut b, 64);
        // cbv payload @56
        w(&mut b, 0); w(&mut b, 0);
        // range @64: type SAMPLER
        w(&mut b, 3); w(&mut b, 1); w(&mut b, 0); w(&mut b, 0); w(&mut b, 0);
        let p = parse_rs(&b).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p[0].kind == 0 && p[0].sampler);
        assert_eq!(p[1].kind, 2);
    }
}
