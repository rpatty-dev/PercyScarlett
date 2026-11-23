use std::fs::OpenOptions;
use std::io::Write;

fn log_stub(name: &str, a0: usize, a1: usize, a2: usize, a3: usize) {
    if let Ok(p) = std::env::var("PERCY_STUB_LOG") {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{name}(0x{a0:x}, 0x{a1:x}, 0x{a2:x}, 0x{a3:x})");
        }
    }
    if let Ok(ab) = std::env::var("PERCY_STUB_ABORT") {
        if !ab.is_empty() && name.contains(&ab) {
            std::process::abort();
        }
    }
}

#[export_name = "??0XboxBvhBuilder@@QEAA@PEAUID3D12Device11@@_N@Z"]
pub unsafe extern "system" fn xbox_bvh_0(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!??0XboxBvhBuilder@@QEAA@PEAUID3D12Device11@@_N@Z", a0, a1, a2, a3);
    0
}

#[export_name = "??1XboxBvhBuilder@@QEAA@XZ"]
pub unsafe extern "system" fn xbox_bvh_1(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!??1XboxBvhBuilder@@QEAA@XZ", a0, a1, a2, a3);
    0
}

#[export_name = "?BuildRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList6@@IPEBUD3D12XBOX_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_BATCHED_DESC2@@@Z"]
pub unsafe extern "system" fn xbox_bvh_2(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!?BuildRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList6@@IPEBUD3D12XBOX_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_BATCHED_DESC2@@@Z", a0, a1, a2, a3);
    0
}

#[export_name = "?CopyRaytracingAccelerationStructure@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList4@@_K1W4D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE@@@Z"]
pub unsafe extern "system" fn xbox_bvh_3(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!?CopyRaytracingAccelerationStructure@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList4@@_K1W4D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE@@@Z", a0, a1, a2, a3);
    0
}

#[export_name = "?GetRaytracingAccelerationStructurePrebuildInfo@XboxBvhBuilder@@QEAAXPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS@@PEBUD3D12XBOX_XDXR_STREAM_DESC@@PEAUD3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO@@@Z"]
pub unsafe extern "system" fn xbox_bvh_4(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!?GetRaytracingAccelerationStructurePrebuildInfo@XboxBvhBuilder@@QEAAXPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS@@PEBUD3D12XBOX_XDXR_STREAM_DESC@@PEAUD3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO@@@Z", a0, a1, a2, a3);
    0
}

#[export_name = "?RehydrateRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAJPEAUID3D12GraphicsCommandList8@@IPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC@@PEB_K@Z"]
pub unsafe extern "system" fn xbox_bvh_5(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!?RehydrateRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAJPEAUID3D12GraphicsCommandList8@@IPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC@@PEB_K@Z", a0, a1, a2, a3);
    0
}

#[export_name = "?Set64BitMortonCodes@XboxBvhBuilder@@QEAAX_N@Z"]
pub unsafe extern "system" fn xbox_bvh_6(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("XboxBvhBuilder.dll!?Set64BitMortonCodes@XboxBvhBuilder@@QEAAX_N@Z", a0, a1, a2, a3);
    0
}
