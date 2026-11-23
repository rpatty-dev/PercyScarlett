//! Console-layout command list (D3D12.X). Titles append records inline at the
//! object's write cursor (+0x18, bounded by +0x30) instead of calling methods; the
//! object fronts a PC ID3D12GraphicsCommandList kept in a hidden header at obj-0x10.
#![allow(dead_code)]
use super::*;

const MAGIC: usize = 0x5453_4c43_5843_5350; // "PSCXCLST"
pub(crate) const OBJ_SIZE: usize = 0x4000;
const BUF_SIZE: usize = 16 << 20;
pub(crate) const OFF_WP: usize = 0x18;
pub(crate) const OFF_LIMIT: usize = 0x28;
pub(crate) const OFF_END: usize = 0x30;

extern "system" {
    fn VirtualAlloc(a: *mut c_void, s: usize, t: u32, p: u32) -> *mut c_void;
    fn VirtualProtect(a: *mut c_void, s: usize, p: u32, old: *mut u32) -> i32;
}

#[repr(C)]
struct Hdr {
    open: usize, // MAGIC | is_open
    inner: *mut c_void,
}

static SEEN: [std::sync::atomic::AtomicBool; 192] = [const { std::sync::atomic::AtomicBool::new(false) }; 192];
fn note(slot: usize, a: [usize; 6]) {
    unsafe { super::replay::flush(a[0] as *mut c_void) };
    unsafe { hist_push(a[0], slot) };
    if !SEEN[slot].swap(true, Ordering::Relaxed) {
        let ra = super::guest_ra();
        extern "system" { fn VirtualQuery(p: *const c_void, mbi: *mut [usize; 6], n: usize) -> usize; }
        let readable = |p: usize| unsafe {
            let mut m = [0usize; 6];
            // MEMORY_BASIC_INFORMATION: State @0x20 (u32), Protect @0x24 (u32)
            VirtualQuery(p as *const c_void, &mut m, 48) != 0 && (m[4] as u32) == 0x1000 && ((m[4] >> 32) as u32) & 0x66 != 0
        };
        let vp = |p: usize| if p > 0x10000 && p & 7 == 0 && readable(p) { unsafe { *(p as *const usize) } } else { 0 };
        log_d3d12(&format!(
            "[clx] slot {slot} from G+{:x} args=({:#x}[vt {:#x}],{:#x},{:#x},{:#x},{:#x})",
            ra.wrapping_sub(0x1_4000_0000), a[1], vp(a[1]), a[2], a[3], a[4], a[5]
        ));
    }
}

#[unsafe(naked)] unsafe extern "system" fn f0() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 0]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 0, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f1() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 8]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 8, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f2() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 16]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 16, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f3() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 24]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 24, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f4() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 32]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 32, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f5() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 40]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 40, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f6() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 48]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 48, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f7() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 56]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 56, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f8() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 64]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 64, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f9() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 72]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 72, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f10() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 80]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 80, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f11() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 88]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 88, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f12() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 96]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 96, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f13() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 104]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 104, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f14() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 112]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 112, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f15() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 120]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 120, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f16() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 128]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 128, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f17() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 136]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 136, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f18() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 144]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 144, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f19() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 152]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 152, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f20() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 160]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 160, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f21() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 168]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 168, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f22() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 176]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 176, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f23() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 184]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 184, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f24() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 192]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 192, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f25() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 200]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 200, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f26() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 208]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 208, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f27() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 216]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 216, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f28() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 224]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 224, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f29() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 232]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 232, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f30() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 240]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 240, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f31() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 248]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 248, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f32() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 256]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 256, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f33() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 264]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 264, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f34() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 272]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 272, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f35() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 280]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 280, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f36() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 288]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 288, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f37() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 296]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 296, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f38() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 304]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 304, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f39() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 312]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 312, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f40() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 320]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 320, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f41() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 328]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 328, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f42() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 336]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 336, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f43() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 344]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 344, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f44() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 352]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 352, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f45() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 360]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 360, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f46() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 368]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 368, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f47() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 376]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 376, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f48() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 384]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 384, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f49() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 392]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 392, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f50() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 400]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 400, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f51() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 408]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 408, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f52() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 416]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 416, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f53() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 424]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 424, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f54() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 432]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 432, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f55() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 440]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 440, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f56() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 448]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 448, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f57() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 456]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 456, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f58() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 464]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 464, c = sym COUNTS); }
#[unsafe(naked)] unsafe extern "system" fn f59() { std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 472]", "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]", o = const 472, c = sym COUNTS); }
unsafe extern "system" fn s60(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(60, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s61(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(61, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s62(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(62, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s63(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(63, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s64(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(64, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s65(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(65, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s66(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(66, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s67(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(67, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s68(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(68, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s69(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(69, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s70(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(70, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s71(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(71, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s72(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(72, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s73(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(73, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s74(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(74, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s75(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(75, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s76(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(76, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s77(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(77, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s78(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(78, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s79(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(79, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s80(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(80, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s81(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(81, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s82(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(82, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s83(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(83, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s84(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(84, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s85(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(85, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s86(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(86, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s87(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(87, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s88(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(88, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s89(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(89, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s90(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(90, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s91(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(91, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s92(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(92, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s93(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(93, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s94(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(94, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s95(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(95, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s96(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(96, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s97(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(97, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s98(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(98, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s99(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(99, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s100(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(100, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s101(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(101, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s102(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(102, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s103(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(103, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s104(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(104, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s105(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(105, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s106(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(106, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s107(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(107, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s108(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(108, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s109(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(109, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s110(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(110, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s111(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(111, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s112(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(112, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s113(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(113, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s114(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(114, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s115(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(115, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s116(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(116, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s117(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(117, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s118(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(118, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s119(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(119, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s120(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(120, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s121(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(121, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s122(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(122, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s123(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(123, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s124(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(124, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s125(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(125, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s126(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(126, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s127(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(127, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s128(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(128, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s129(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(129, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s130(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(130, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s131(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(131, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s132(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(132, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s133(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(133, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s134(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(134, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s135(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(135, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s136(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(136, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s137(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(137, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s138(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(138, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s139(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(139, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s140(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(140, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s141(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(141, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s142(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(142, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s143(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(143, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s144(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(144, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s145(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(145, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s146(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(146, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s147(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(147, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s148(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(148, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s149(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(149, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s150(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(150, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s151(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(151, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s152(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(152, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s153(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(153, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s154(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(154, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s155(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(155, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s156(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(156, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s157(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(157, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s158(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(158, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s159(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(159, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s160(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(160, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s161(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(161, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s162(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(162, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s163(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(163, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s164(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(164, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s165(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(165, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s166(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(166, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s167(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(167, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s168(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(168, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s169(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(169, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s170(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(170, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s171(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(171, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s172(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(172, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s173(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(173, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s174(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(174, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s175(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(175, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s176(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(176, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s177(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(177, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s178(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(178, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s179(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(179, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s180(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(180, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s181(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(181, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s182(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(182, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s183(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(183, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s184(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(184, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s185(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(185, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s186(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(186, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s187(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(187, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s188(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(188, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s189(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(189, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s190(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(190, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s191(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(191, [a, b, c, d, e, f]); 0 }

unsafe extern "system" fn s11(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(11, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s12(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(12, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s13(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(13, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s14(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(14, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s15(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(15, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s16(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(16, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s17(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(17, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s18(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(18, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s19(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(19, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s20(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(20, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s21(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(21, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s22(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(22, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s23(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(23, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s24(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(24, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s25(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(25, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s26(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(26, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s27(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(27, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s28(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(28, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s29(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(29, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s30(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(30, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s31(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(31, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s32(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(32, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s33(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(33, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s34(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(34, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s35(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(35, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s36(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(36, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s37(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(37, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s38(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(38, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s39(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(39, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s40(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(40, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s41(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(41, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s42(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(42, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s43(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(43, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s44(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(44, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s45(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(45, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s46(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(46, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s47(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(47, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s48(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(48, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s49(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(49, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s50(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(50, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s51(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(51, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s52(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(52, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s53(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(53, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s54(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(54, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s55(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(55, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s56(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(56, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s57(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(57, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s58(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(58, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn s59(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { note(59, [a, b, c, d, e, f]); 0 }
const LOGONLY: [unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) -> usize; 49] = [s11, s12, s13, s14, s15, s16, s17, s18, s19, s20, s21, s22, s23, s24, s25, s26, s27, s28, s29, s30, s31, s32, s33, s34, s35, s36, s37, s38, s39, s40, s41, s42, s43, s44, s45, s46, s47, s48, s49, s50, s51, s52, s53, s54, s55, s56, s57, s58, s59];
const FWD: [unsafe extern "system" fn(); 60] = [f0, f1, f2, f3, f4, f5, f6, f7, f8, f9, f10, f11, f12, f13, f14, f15, f16, f17, f18, f19, f20, f21, f22, f23, f24, f25, f26, f27, f28, f29, f30, f31, f32, f33, f34, f35, f36, f37, f38, f39, f40, f41, f42, f43, f44, f45, f46, f47, f48, f49, f50, f51, f52, f53, f54, f55, f56, f57, f58, f59];
const STUBS: [unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) -> usize; 132] = [s60, s61, s62, s63, s64, s65, s66, s67, s68, s69, s70, s71, s72, s73, s74, s75, s76, s77, s78, s79, s80, s81, s82, s83, s84, s85, s86, s87, s88, s89, s90, s91, s92, s93, s94, s95, s96, s97, s98, s99, s100, s101, s102, s103, s104, s105, s106, s107, s108, s109, s110, s111, s112, s113, s114, s115, s116, s117, s118, s119, s120, s121, s122, s123, s124, s125, s126, s127, s128, s129, s130, s131, s132, s133, s134, s135, s136, s137, s138, s139, s140, s141, s142, s143, s144, s145, s146, s147, s148, s149, s150, s151, s152, s153, s154, s155, s156, s157, s158, s159, s160, s161, s162, s163, s164, s165, s166, s167, s168, s169, s170, s171, s172, s173, s174, s175, s176, s177, s178, s179, s180, s181, s182, s183, s184, s185, s186, s187, s188, s189, s190, s191];

pub(crate) static mut COUNTS: [u64; 64] = [0; 64];
pub(crate) unsafe fn census() -> String {
    let mut s = String::new();
    for i in 0..64 {
        let c = std::ptr::read_volatile(std::ptr::addr_of!(COUNTS[i]));
        if c > 0 { s.push_str(&format!(" {i}:{c}")); }
    }
    s
}

static LT_N: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
unsafe fn hist_push(obj: usize, slot: usize) {
    if obj & 0xfff == 0xff8 || inner_of(obj as *mut c_void).is_none() {
        return;
    }
    let base = (obj + OBJ_SIZE - 0x50) as *mut usize;
    let i = *base;
    *base.add(1 + (i & 7)) = slot;
    *base = i + 1;
}
unsafe fn hist(obj: *mut c_void) -> String {
    let base = ((obj as usize) + OBJ_SIZE - 0x50) as *const usize;
    let n = *base;
    (n.saturating_sub(8)..n).map(|k| format!("{}", *base.add(1 + (k & 7)))).collect::<Vec<_>>().join(",")
}
extern "system" fn lt_log(slot: usize, this: usize, a1: usize, a2: usize, a3: usize, ra: usize) {
    unsafe { super::replay::flush(this as *mut c_void) };
    unsafe { hist_push(this, slot) };
    if LT_N[slot].fetch_add(1, Ordering::Relaxed) < 3 {
        log_d3d12(&format!("[clx] s{slot} ({a1:#x}, {a2:#x}, {a3:#x}) from G+{:x}", ra.wrapping_sub(0x1_4000_0000)));
    }
}
/// Logging forwarder: preserves rcx/rdx/r8/r9 + xmm0-3, logs, then tail-jumps to the PC slot.
macro_rules! lt {
    ($name:ident, $slot:literal, $pc:literal) => {
        #[unsafe(naked)]
        unsafe extern "system" fn $name() {
            std::arch::naked_asm!(
                "push rcx", "push rdx", "push r8", "push r9", "sub rsp, 0x78",
                "movdqu [rsp + 0x30], xmm0", "movdqu [rsp + 0x40], xmm1", "movdqu [rsp + 0x50], xmm2", "movdqu [rsp + 0x60], xmm3",
                "mov rax, [rsp + 0x98]", "mov [rsp + 0x28], rax",
                "mov [rsp + 0x20], r9", "mov r9, r8", "mov r8, rdx", "mov rdx, rcx", "mov ecx, {s}",
                "call {log}",
                "movdqu xmm0, [rsp + 0x30]", "movdqu xmm1, [rsp + 0x40]", "movdqu xmm2, [rsp + 0x50]", "movdqu xmm3, [rsp + 0x60]",
                "add rsp, 0x78", "pop r9", "pop r8", "pop rdx", "pop rcx",
                "mov rcx, qword ptr [rcx - 8]", "mov rax, qword ptr [rcx]", "jmp qword ptr [rax + {o}]",
                s = const $slot, log = sym lt_log, o = const $pc * 8,
            );
        }
    };
}
lt!(l11, 11, 11);
lt!(l12, 12, 12);
lt!(l13, 13, 13);
lt!(l14, 14, 14);
lt!(l15, 15, 15);
lt!(l16, 16, 16);
lt!(l17, 17, 17);
lt!(l18, 18, 18);
lt!(l19, 19, 19);
lt!(l20, 20, 20);
lt!(l21, 21, 21);
lt!(l22, 22, 22);
lt!(l23, 23, 23);
lt!(l24, 24, 24);
lt!(l25, 25, 25);
lt!(l26, 26, 26);
lt!(l31, 31, 27);
lt!(l32, 32, 28);
lt!(l33, 33, 29);
lt!(l34, 34, 30);
lt!(l35, 35, 31);
lt!(l36, 36, 32);
lt!(l37, 37, 33);
lt!(l38, 38, 34);
lt!(l39, 39, 35);
lt!(l40, 40, 36);
lt!(l41, 41, 37);
lt!(l42, 42, 38);
lt!(l43, 43, 39);
lt!(l44, 44, 40);
lt!(l45, 45, 41);
lt!(l46, 46, 42);
lt!(l47, 47, 43);
lt!(l48, 48, 44);
lt!(l49, 49, 45);
lt!(l50, 50, 46);
lt!(l51, 51, 47);
lt!(l52, 52, 48);
lt!(l53, 53, 49);
lt!(l54, 54, 50);
lt!(l55, 55, 51);
lt!(l56, 56, 52);
lt!(l57, 57, 53);
lt!(l58, 58, 54);
lt!(l59, 59, 55);
const LT_LO: [unsafe extern "system" fn(); 16] = [l11, l12, l13, l14, l15, l16, l17, l18, l19, l20, l21, l22, l23, l24, l25, l26];
const LT: [unsafe extern "system" fn(); 29] = [l31, l32, l33, l34, l35, l36, l37, l38, l39, l40, l41, l42, l43, l44, l45, l46, l47, l48, l49, l50, l51, l52, l53, l54, l55, l56, l57, l58, l59];

static VTBL: OnceLock<usize> = OnceLock::new();

fn vtbl() -> usize {
    *VTBL.get_or_init(|| {
        let mut v = [0usize; 192];
        for i in 0..60 {
            v[i] = FWD[i] as usize;
        }
        for i in 60..192 {
            v[i] = STUBS[i - 60] as usize;
        }
        if std::env::current_exe().map(|p| p.with_file_name("percy_clx_map").exists()).unwrap_or(false) {
            for i in 11..60 {
                v[i] = LOGONLY[i - 11] as usize;
            }
        }
        for i in 11..27 {
            v[i] = LT_LO[i - 11] as usize;
        }
        for i in 31..60 {
            v[i] = LT[i - 31] as usize;
        }
        // Console 27..30: console-only methods (PC slots shift by 4 from here); log and drop.
        for i in 27..31 {
            v[i] = LOGONLY[i - 11] as usize;
        }
        v[26] = cl_resource_barrier as usize;
        v[19] = cl_resolve_subresource as usize;
        v[32] = cl_set_descriptor_heaps as usize;
        v[25] = cl_set_pso as usize;
        v[33] = cl_set_cs_rs as usize;
        v[83] = cl_copy_texture_region as usize;
        v[15] = cl_copy_buffer_region as usize;
        v[17] = cl_copy_resource as usize;
        v[34] = cl_set_gfx_rs as usize;
        v[9] = cl_close as usize;
        v[10] = cl_reset as usize;
        v[139] = cl_reset_x as usize;
        // newer layout, base #69-#72 shifted by two (xcl): bottom-of-pipe writes and memory waits
        v[71] = super::cpu_writes::write32_bottom as usize;
        v[72] = super::cpu_writes::write64_bottom as usize;
        v[73] = super::cpu_writes::wait32 as usize;
        v[74] = super::cpu_writes::wait64 as usize;
        if let Ok(s) = std::fs::read_to_string(std::env::current_exe().unwrap_or_default().with_file_name("percy_clx_drop.txt")) {
            for i in s.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter(|&i| (11..60).contains(&i)) {
                v[i] = LOGONLY[i - 11] as usize;
            }
        }
        v[50] = cl_om_set_rts as usize;
        v[51] = cl_clear_dsv as usize;
        v[52] = cl_clear_rtv as usize;
        Box::leak(Box::new(v)).as_ptr() as usize
    })
}

pub(crate) unsafe fn inner_of(obj: *mut c_void) -> Option<*mut c_void> {
    if obj.is_null() {
        return None;
    }
    let h = (obj as *const u8).sub(16) as *const Hdr;
    if (*h).open & !3 == MAGIC { Some((*h).inner) } else { None }
}

pub(crate) unsafe fn buf_start(obj: *mut c_void) -> *mut u8 {
    *((obj as *const u8).add(OBJ_SIZE - 8) as *const *mut u8)
}

pub(crate) unsafe fn rewind(obj: *mut c_void) {
    let o = obj as *mut u8;
    let b = buf_start(obj);
    *(o.add(OFF_WP) as *mut *mut u8) = b;
    *(o.add(OFF_LIMIT) as *mut *mut u8) = b.add(BUF_SIZE);
    *(o.add(OFF_END) as *mut *mut u8) = b.add(BUF_SIZE);
}

/// Wrap a PC command list in a console-layout object.
pub(crate) unsafe fn wrap(inner: *mut c_void) -> *mut c_void {
    let probe = std::env::current_exe().map(|p| p.with_file_name("percy_clx_probe").exists()).unwrap_or(false);
    if probe {
        // vtable ptr = last 8 bytes of page 0; every other field lives in no-access page 1.
        let blk = VirtualAlloc(std::ptr::null_mut(), 0x2000 + OBJ_SIZE, 0x3000, 0x04) as *mut u8;
        let obj = blk.add(0x1000 - 8);
        let h = obj.sub(16) as *mut Hdr;
        (*h).open = MAGIC | 1;
        (*h).inner = inner;
        *(obj as *mut usize) = vtbl();
        let mut old = 0u32;
        VirtualProtect(blk.add(0x1000) as *mut c_void, OBJ_SIZE, 0x01, &mut old);
        log_d3d12(&format!("[clx] probe obj={obj:p}"));
        return obj as *mut c_void;
    }
    let block = VirtualAlloc(std::ptr::null_mut(), OBJ_SIZE + 16, 0x3000, 0x04) as *mut u8;
    let buf = VirtualAlloc(std::ptr::null_mut(), BUF_SIZE, 0x3000, 0x04) as *mut u8;
    if block.is_null() || buf.is_null() {
        return inner;
    }
    let h = block as *mut Hdr;
    (*h).open = MAGIC | 1;
    (*h).inner = inner;
    let obj = block.add(16) as *mut c_void;
    *(obj as *mut usize) = vtbl();
    *((obj as *mut u8).add(OBJ_SIZE - 8) as *mut *mut u8) = buf;
    rewind(obj);
    obj
}

static CLOSES: AtomicU32 = AtomicU32::new(0);

static RESETS: AtomicU32 = AtomicU32::new(0);

unsafe fn dump(obj: *mut c_void, tag: &str) {
    if (obj as usize) & 0xfff == 0xff8 {
        return; // guard-page probe object: no fields readable
    }
    let b = buf_start(obj);
    let wp = *((obj as *const u8).add(OFF_WP) as *const *mut u8);
    let used = (wp as usize).wrapping_sub(b as usize);
    if used == 0 || used > BUF_SIZE {
        return;
    }
    let n = CLOSES.fetch_add(1, Ordering::Relaxed);
    if n >= 6 {
        return;
    }
    if let Ok(p) = std::env::current_exe() {
        let _ = std::fs::write(p.with_file_name(format!("clx_dump_{n}.bin")), std::slice::from_raw_parts(b, used));
    }
    let words = std::slice::from_raw_parts(b as *const u32, (used / 4).min(24));
    let hex: Vec<String> = words.iter().map(|w| format!("{w:08x}")).collect();
    log_d3d12(&format!("[clx] {tag} obj={obj:p} used={used:#x} {}", hex.join(" ")));
}

/// Close failed: the PC list is in an error state and must not be submitted.
pub(crate) unsafe fn failed(obj: *mut c_void) -> bool {
    (*((obj as *const u8).sub(16) as *const Hdr)).open & 2 != 0
}

unsafe fn set_open(obj: *mut c_void, open: bool) {
    let h = (obj as *mut u8).sub(16) as *mut Hdr;
    (*h).open = MAGIC | open as usize;
}

/// PC lists must be closed before submission; console titles may submit open lists.
pub(crate) unsafe fn ensure_closed(obj: *mut c_void) {
    let h = (obj as *mut u8).sub(16) as *mut Hdr;
    if (*h).open & 1 != 0 {
        static N: AtomicU32 = AtomicU32::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 3 {
            log_d3d12(&format!("[clx] implicit Close before Execute obj={obj:p}"));
        }
        cl_close(obj);
    }
}

unsafe extern "system" fn cl_close(obj: *mut c_void) -> i32 {
    let h = (obj as *mut u8).sub(16) as *mut Hdr;
    if (*h).open & 1 == 0 {
        return 0; // already closed: console Close is idempotent
    }
    super::replay::flush(obj);
    set_open(obj, false);
    dump(obj, "Close");
    let inner = inner_of(obj).unwrap_or(obj);
    let f: unsafe extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(*(*(inner as *const *const usize)).add(9));
    let hr = f(inner);
    if hr < 0 {
        static CF: AtomicU32 = AtomicU32::new(0);
        let k = CF.fetch_add(1, Ordering::Relaxed);
        if k < 12 || k.is_power_of_two() {
            log_d3d12(&format!("[clx] Close obj={obj:p} -> {:08x} last=[{}] n={k}", hr as u32, hist(obj)));
            drain_info("close", 8);
        }
        let h = (obj as *mut u8).sub(16) as *mut Hdr;
        (*h).open = MAGIC | 2;
    }
    hr
}

/// Console slot 139 (+0x458) ResetX(allocator, initialPso, flags): the title resets lists only here.
unsafe extern "system" fn cl_reset_x(obj: *mut c_void, alloc: *mut c_void, pso: *mut c_void, _flags: u32) -> i32 { cl_reset(obj, alloc, pso) }
unsafe extern "system" fn cl_reset(obj: *mut c_void, alloc: *mut c_void, pso: *mut c_void) -> i32 {
    let inner = inner_of(obj).unwrap_or(obj);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
        std::mem::transmute(*(*(inner as *const *const usize)).add(10));
    set_open(obj, true);
    super::cpu_writes::reset(obj);
    if RESETS.fetch_add(1, Ordering::Relaxed) < 6 {
        log_d3d12(&format!("[clx] Reset obj={obj:p}"));
    }
    let pso = super::replay::unwrap_pso(pso);
    if (obj as usize) & 0xfff != 0xff8 {
        rewind(obj);
        super::replay::reset_state(obj);
    }
    let mut hr = f(inner, alloc, pso);
    if hr < 0 {
        let hr0 = hr;
        if !pso.is_null() {
            hr = f(inner, alloc, std::ptr::null_mut());
        }
        static RF: AtomicU32 = AtomicU32::new(0);
        if RF.fetch_add(1, Ordering::Relaxed) < 8 {
            let vt = |p: *mut c_void| if p.is_null() { 0 } else { *(p as *const usize) };
            log_d3d12(&format!(
                "[clx] Reset obj={obj:p} alloc={alloc:p}(vt {:#x}) pso={pso:p}(vt {:#x}) -> {:08x}; null-pso retry -> {:08x}",
                vt(alloc), vt(pso), hr0 as u32, hr as u32
            ));
        }
    }
    hr
}

// ---- console command queue: PC slots 0..19 + console-only slots 19..63 ----
static QSEEN: [std::sync::atomic::AtomicBool; 64] = [const { std::sync::atomic::AtomicBool::new(false) }; 64];
fn qnote(slot: usize, a: [usize; 6]) {
    if !QSEEN[slot].swap(true, Ordering::Relaxed) {
        let rsp: usize;
        unsafe { std::arch::asm!("mov {}, rsp", out(reg) rsp) };
        let ra = (0..96)
            .map(|i| unsafe { *((rsp as *const usize).add(i)) })
            .find(|&v| (0x1_4000_1000..0x1_4c00_0000).contains(&v))
            .unwrap_or(0);
        log_d3d12(&format!(
            "[qx] slot {slot} (unimpl) from G+{:x} args=({:#x},{:#x},{:#x},{:#x},{:#x})",
            ra.wrapping_sub(0x1_4000_0000), a[1], a[2], a[3], a[4], a[5]
        ));
    }
}
unsafe extern "system" fn q19(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(19, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q20(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(20, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q21(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(21, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q22(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(22, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q23(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(23, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q24(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(24, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q25(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(25, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q26(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(26, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q27(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(27, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q28(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(28, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q29(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(29, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q30(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(30, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q31(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(31, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q32(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(32, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q33(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(33, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q34(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(34, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q35(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(35, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q36(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(36, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q37(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(37, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q38(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(38, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q39(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(39, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q40(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(40, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q41(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(41, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q42(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(42, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q43(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(43, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q44(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(44, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q45(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(45, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q46(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(46, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q47(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(47, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q48(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(48, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q49(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(49, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q50(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(50, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q51(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(51, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q52(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(52, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q53(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(53, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q54(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(54, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q55(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(55, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q56(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(56, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q57(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(57, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q58(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(58, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q59(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(59, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q60(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(60, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q61(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(61, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q62(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(62, [a, b, c, d, e, f]); 0 }
unsafe extern "system" fn q63(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize { qnote(63, [a, b, c, d, e, f]); 0 }
const QSTUBS: [unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) -> usize; 45] = [q19, q20, q21, q22, q23, q24, q25, q26, q27, q28, q29, q30, q31, q32, q33, q34, q35, q36, q37, q38, q39, q40, q41, q42, q43, q44, q45, q46, q47, q48, q49, q50, q51, q52, q53, q54, q55, q56, q57, q58, q59, q60, q61, q62, q63];
const PC_QUEUE_SLOTS: usize = 19;
static QTABLES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new()); // (pc vtbl, console vtbl)

/// Give a PC queue a console-sized vtable; overrides replace (slot, fn).
pub(crate) unsafe fn patch_queue(q: *mut c_void, overrides: &[(usize, usize)]) {
    let vp = q as *mut usize;
    let orig = *vp;
    let mut t = QTABLES.lock().unwrap();
    if t.iter().any(|&(_, n)| n == orig) {
        return;
    }
    let n = match t.iter().find(|&&(o, _)| o == orig) {
        Some(&(_, n)) => n,
        None => {
            let mut v = vec![0usize; 64];
            v[..PC_QUEUE_SLOTS].copy_from_slice(std::slice::from_raw_parts(orig as *const usize, PC_QUEUE_SLOTS));
            for i in PC_QUEUE_SLOTS..64 {
                v[i] = QSTUBS[i - PC_QUEUE_SLOTS] as usize;
            }
            for &(s, f) in overrides {
                v[s] = f;
            }
            let n = Box::leak(v.into_boxed_slice()).as_ptr() as usize;
            t.push((orig, n));
            n
        }
    };
    *vp = n;
}

/// Original PC method for a patched queue.
pub(crate) unsafe fn queue_orig(q: *mut c_void, slot: usize) -> usize {
    let cur = *(q as *const usize);
    let t = QTABLES.lock().unwrap();
    let pc = t.iter().find(|&&(_, n)| n == cur).map(|&(o, _)| o).unwrap_or(cur);
    *(pc as *const usize).add(slot)
}

unsafe fn inner_fn(obj: *mut c_void, slot: usize) -> (*mut c_void, usize) {
    let inner = inner_of(obj).unwrap_or(obj);
    (inner, *(*(inner as *const *const usize)).add(slot))
}

/// Slot 26: ResourceBarrier(n, barriers[32 B]). Console allows buffer-only state bits
/// (VERTEX_AND_CONSTANT_BUFFER | INDEX_BUFFER) on textures; PC rejects the list at Close.
unsafe extern "system" fn cl_resource_barrier(obj: *mut c_void, n: u32, b: *const u8) {
    super::replay::flush(obj);
    hist_push(obj as usize, 26);
    let mut v: Vec<[u8; 32]> = (0..n as usize).map(|i| *(b.add(i * 32) as *const [u8; 32])).collect();
    let mut fixed = 0;
    for e in v.iter_mut() {
        if *(e.as_ptr() as *const u32) != 0 {
            continue; // only TRANSITION carries states
        }
        let r = *(e.as_ptr().add(8) as *const *mut c_void);
        if r.is_null() {
            continue;
        }
        if let Some(t) = super::mirror::twin_of(r) {
            *(e.as_mut_ptr().add(8) as *mut *mut c_void) = t;
            continue;
        }
        let gd: unsafe extern "system" fn(*mut c_void, *mut [u64; 7]) -> *mut [u64; 7] = std::mem::transmute(*(*(r as *const *const usize)).add(10));
        let mut d = [0u64; 7];
        gd(r, &mut d);
        if d[0] as u32 == 1 {
            continue; // buffer
        }
        for off in [20usize, 24] {
            let s = e.as_mut_ptr().add(off) as *mut u32;
            if *s & 0x3 != 0 {
                *s &= !0x3;
                fixed += 1;
            }
        }
    }
    static N: AtomicU32 = AtomicU32::new(0);
    if fixed > 0 && N.fetch_add(1, Ordering::Relaxed) < 3 {
        log_d3d12(&format!("[clx] barrier: stripped buffer-only states on {fixed} texture transitions"));
    }
    let (inner, f) = inner_fn(obj, 26);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8) = std::mem::transmute(f);
    f(inner, n, v.as_ptr() as *const u8)
}

unsafe fn res_desc(r: *mut c_void) -> [u64; 7] {
    let mut d = [0u64; 7];
    if !r.is_null() {
        let gd: unsafe extern "system" fn(*mut c_void, *mut [u64; 7]) -> *mut [u64; 7] = std::mem::transmute(*(*(r as *const *const usize)).add(10));
        gd(r, &mut d);
    }
    d
}

/// Slot 19: ResolveSubresource(dst, dstSub, src, srcSub, fmt). Console titles resolve into
/// targets of a different size; PC needs equal sizes, so clamp via ResolveSubresourceRegion (64).
unsafe extern "system" fn cl_resolve_subresource(obj: *mut c_void, dst: *mut c_void, dsub: u32, src: *mut c_void, ssub: u32, fmt: u32) {
    super::replay::flush(obj);
    hist_push(obj as usize, 19);
    let (dd, sd) = (res_desc(dst), res_desc(src));
    let (dw, dh, sw, sh) = (dd[2] as u32, dd[3] as u32, sd[2] as u32, sd[3] as u32);
    if dw == sw && dh == sh {
        let (inner, f) = inner_fn(obj, 19);
        let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void, u32, u32) = std::mem::transmute(f);
        return f(inner, dst, dsub, src, ssub, fmt);
    }
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 2 {
        log_d3d12(&format!("[clx] Resolve {sw}x{sh} -> {dw}x{dh}: clamped region"));
    }
    let rect: [i32; 4] = [0, 0, sw.min(dw) as i32, sh.min(dh) as i32];
    let (inner, f) = inner_fn(obj, 64);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, u32, *mut c_void, u32, *const i32, u32, u32) = std::mem::transmute(f);
    f(inner, dst, dsub, 0, 0, src, ssub, rect.as_ptr(), fmt, 3) // D3D12_RESOLVE_MODE_AVERAGE
}

/// Console slot 32 = SetDescriptorHeaps(n, heaps) (PC 28) — logs heap types.
unsafe extern "system" fn cl_set_descriptor_heaps(obj: *mut c_void, n: u32, heaps: *const *mut c_void) {
    super::replay::flush(obj);
    super::replay::set_heaps(obj, n, heaps);
    hist_push(obj as usize, 32);
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 4 {
        let s: Vec<String> = (0..n as usize).map(|i| {
            let h = *heaps.add(i);
            if h.is_null() { return "null".into(); }
            let gd: unsafe extern "system" fn(*mut c_void, *mut [u32; 4]) -> *mut [u32; 4] = std::mem::transmute(*(*(h as *const *const usize)).add(8));
            let mut d = [0u32; 4];
            gd(h, &mut d);
            format!("t{}/n{}/f{:x}", d[0], d[1], d[2])
        }).collect();
        log_d3d12(&format!("[clx] SetDescriptorHeaps {}", s.join(" ")));
    }
    let (inner, f) = inner_fn(obj, 28);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = std::mem::transmute(f);
    f(inner, n, heaps)
}

/// Console slot 52 = ClearRenderTargetView(rtv, color[4], nRects, rects) (PC slot 48). Title RTV
/// handles point into title-placed heaps: translate to the host heap.
unsafe extern "system" fn cl_clear_rtv(obj: *mut c_void, rtv: usize, color: *const f32, n: u32, rects: *const c_void) {
    super::replay::flush(obj);
    let h = super::xlate_cpu(rtv);
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 4 && !color.is_null() {
        log_d3d12(&format!("[clx] ClearRTV {rtv:#x}->{h:#x} rgba=({},{},{},{}) n={n}", *color, *color.add(1), *color.add(2), *color.add(3)));
    }
    let (inner, f) = inner_fn(obj, 48);
    let f: unsafe extern "system" fn(*mut c_void, usize, *const f32, u32, *const c_void) = std::mem::transmute(f);
    f(inner, h, color, n, rects)
}
/// Console slot 51 = ClearDepthStencilView(dsv, flags, depth, stencil, nRects, rects) (PC 47).
unsafe extern "system" fn cl_clear_dsv(obj: *mut c_void, dsv: usize, flags: u32, depth: f32, stencil: u8, n: u32, rects: *const c_void) {
    super::replay::flush(obj);
    let (inner, f) = inner_fn(obj, 47);
    let f: unsafe extern "system" fn(*mut c_void, usize, u32, f32, u8, u32, *const c_void) = std::mem::transmute(f);
    f(inner, super::xlate_cpu(dsv), flags, depth, stencil, n, rects)
}
/// Console slot 50 = OMSetRenderTargets(n, rtvs, singleRange, dsv) (PC 46).
unsafe extern "system" fn cl_om_set_rts(obj: *mut c_void, n: u32, rtvs: *const usize, single: i32, dsv: *const usize) {
    super::replay::flush(obj);
    let k = if single != 0 { n.min(1) } else { n } as usize;
    let r: Vec<usize> = if rtvs.is_null() { vec![] } else { (0..k).map(|i| super::xlate_cpu(*rtvs.add(i))).collect() };
    let d = if dsv.is_null() { 0 } else { super::xlate_cpu(*dsv) };
    let (inner, f) = inner_fn(obj, 46);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const usize, i32, *const usize) = std::mem::transmute(f);
    f(inner, n, if rtvs.is_null() { rtvs } else { r.as_ptr() }, single, if dsv.is_null() { dsv } else { &d })
}

/// Console slot 25 = SetPipelineState (PC 25): method-call fallback of the inline record.
unsafe extern "system" fn cl_set_pso(obj: *mut c_void, pso: *mut c_void) {
    super::replay::flush(obj);
    let (inner, f) = inner_fn(obj, 25);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(f);
    f(inner, super::replay::unwrap_pso(pso))
}

/// Console slot 33/34 = SetCompute/GraphicsRootSignature (PC 29/30).
unsafe extern "system" fn cl_set_cs_rs(obj: *mut c_void, rs: *mut c_void) {
    super::replay::flush(obj);
    super::replay::set_root_signature(obj, rs, true);
    let (inner, f) = inner_fn(obj, 29);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(f);
    f(inner, rs)
}
thread_local! {
    /// Debug: which PC call cl_set_gfx_rs is in (1 flush, 2 set_rs, 3 PC SetGraphicsRootSignature), with obj/rs.
    pub(crate) static GFX_RS_STEP: std::cell::Cell<(u32, usize, usize)> = const { std::cell::Cell::new((0, 0, 0)) };
}
unsafe extern "system" fn cl_set_gfx_rs(obj: *mut c_void, rs: *mut c_void) {
    let st = |n| GFX_RS_STEP.with(|c| c.set((n, obj as usize, rs as usize)));
    st(1);
    super::replay::flush(obj);
    st(2);
    super::replay::set_root_signature(obj, rs, false);
    let (inner, f) = inner_fn(obj, 30);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(f);
    st(3);
    f(inner, rs);
    st(0);
}

/// Slot 15: CopyBufferRegion(dst, dstOff, src, srcOff, bytes) — mirrored buffers use their twin.
unsafe extern "system" fn cl_copy_buffer_region(obj: *mut c_void, dst: *mut c_void, doff: u64, src: *mut c_void, soff: u64, n: u64) {
    super::replay::flush(obj);
    hist_push(obj as usize, 15);
    let (inner, f) = inner_fn(obj, 15);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u64, *mut c_void, u64, u64) = std::mem::transmute(f);
    f(inner, super::mirror::sub(dst), doff, super::mirror::sub(src), soff, n)
}
/// Slot 17: CopyResource(dst, src).
unsafe extern "system" fn cl_copy_resource(obj: *mut c_void, dst: *mut c_void, src: *mut c_void) {
    super::replay::flush(obj);
    hist_push(obj as usize, 17);
    let (inner, f) = inner_fn(obj, 17);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) = std::mem::transmute(f);
    f(inner, super::mirror::sub(dst), super::mirror::sub(src))
}

/// Console slot 83: CopyTextureRegion(dst, x, y, z, src, box, flags) -> PC 16 (flags dropped).
unsafe extern "system" fn cl_copy_texture_region(obj: *mut c_void, dst: *const u8, x: u32, y: u32, z: u32, src: *const u8, bx: *const u32, flags: u32) {
    super::replay::flush(obj);
    hist_push(obj as usize, 83);
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 3 {
        log_d3d12(&format!("[clx] CopyTextureRegion dst type {} src type {} flags {flags:#x}", *(dst.add(8) as *const u32), *(src.add(8) as *const u32)));
    }
    let (inner, f) = inner_fn(obj, 16);
    let f: unsafe extern "system" fn(*mut c_void, *const u8, u32, u32, u32, *const u8, *const u32) = std::mem::transmute(f);
    f(inner, dst, x, y, z, src, bx)
}

/// Console slots 29/30: fence operations recorded in the list, (fence, value, ...). A PC list
/// cannot signal; the queue signals them after the list executes.
static FENCE_OPS: Mutex<Vec<(usize, usize, usize, u64)>> = Mutex::new(Vec::new()); // (list, slot, fence, value)
unsafe fn fence_op(obj: *mut c_void, slot: usize, fence: usize, value: u64) {
    super::replay::flush(obj);
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 6 {
        log_d3d12(&format!("[clx] fence op {slot} fence={fence:#x} value={value}"));
    }
    FENCE_OPS.lock().unwrap().push((obj as usize, slot, fence, value));
}
unsafe extern "system" fn cl_fence_op29(obj: *mut c_void, fence: usize, value: u64) { fence_op(obj, 29, fence, value) }
unsafe extern "system" fn cl_fence_op30(obj: *mut c_void, fence: usize, value: u64) { fence_op(obj, 30, fence, value) }

/// After ExecuteCommandLists: signal the fences the submitted lists recorded.
pub(crate) unsafe fn signal_fences(q: *mut c_void, n: u32, lists: *const *mut c_void) {
    let ids: Vec<usize> = (0..n as usize).map(|i| *lists.add(i) as usize).collect();
    let ops: Vec<(usize, usize, usize, u64)> = {
        let mut v = FENCE_OPS.lock().unwrap();
        let (mine, rest): (Vec<_>, Vec<_>) = v.drain(..).partition(|o| ids.contains(&o.0));
        *v = rest;
        mine
    };
    let sig: unsafe extern "system" fn(*mut c_void, usize, u64) -> i32 = std::mem::transmute(queue_orig(q, 14));
    // Slots 29/30 turned out to take a query heap (timestamps), not a fence: nothing to signal.
    for (_, slot, fence, value) in ops.into_iter().filter(|_| false) {
        let hr = sig(q, fence, value);
        static N: AtomicU32 = AtomicU32::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 6 {
            log_d3d12(&format!("[clx] queue Signal (op {slot}) fence={fence:#x} value={value} -> {:08x}", hr as u32));
        }
    }
}
