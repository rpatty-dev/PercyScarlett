//! Clean-room hardware D3D12 compute shader runner for verifying lifted RDNA2 DXIL output.
//! Executes real SM 6.6 DXIL compute pipelines on the host GPU and reads back UAV memory.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    const S_OK: i32 = 0;

    // ID3D12Device: {189819f1-1db6-4b57-be54-1821339b85f7}
    const GUID_ID3D12_DEVICE: [u8; 16] = [
        0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7,
    ];
    // ID3D12CommandQueue: {0ec870a6-5d7e-4c22-8cfc-5baae07616ed}
    const GUID_ID3D12_COMMAND_QUEUE: [u8; 16] = [
        0xa6, 0x70, 0xc8, 0x0e, 0x7e, 0x5d, 0x22, 0x4c, 0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed,
    ];
    // ID3D12CommandAllocator: {6102dee4-af59-4b09-b999-b44d73f09b24}
    const GUID_ID3D12_COMMAND_ALLOCATOR: [u8; 16] = [
        0xe4, 0xde, 0x02, 0x61, 0x59, 0xaf, 0x09, 0x4b, 0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24,
    ];
    // ID3D12GraphicsCommandList: {5b160d0f-ac1b-4185-8ba8-b3ae42a5a455}
    const GUID_ID3D12_GRAPHICS_COMMAND_LIST: [u8; 16] = [
        0x0f, 0x0d, 0x16, 0x5b, 0x1b, 0xac, 0x85, 0x41, 0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55,
    ];
    // ID3D12RootSignature: {c54a6b66-72df-4ee8-8be5-a946a1429214}
    const GUID_ID3D12_ROOT_SIGNATURE: [u8; 16] = [
        0x66, 0x6b, 0x4a, 0xc5, 0xdf, 0x72, 0xe8, 0x4e, 0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14,
    ];
    // ID3D12PipelineState: {765a30f3-f624-4c6f-a828-ace948622445}
    const GUID_ID3D12_PIPELINE_STATE: [u8; 16] = [
        0xf3, 0x30, 0x5a, 0x76, 0x24, 0xf6, 0x6f, 0x4c, 0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45,
    ];
    // ID3D12Resource: {696442be-a72e-4059-bc79-5b5c98040fad}
    const GUID_ID3D12_RESOURCE: [u8; 16] = [
        0xbe, 0x42, 0x64, 0x69, 0x2e, 0xa7, 0x59, 0x40, 0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad,
    ];
    // ID3D12Fence: {0a753dcf-c4d8-4b91-adf6-be5a60d95a76}
    const GUID_ID3D12_FENCE: [u8; 16] = [
        0xcf, 0x3d, 0x75, 0x0a, 0xd8, 0xc4, 0x91, 0x4b, 0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76,
    ];

    #[repr(C)]
    struct CommandQueueDesc {
        ty: u32,
        priority: i32,
        flags: u32,
        node_mask: u32,
    }

    #[repr(C)]
    struct RootDescriptor {
        shader_register: u32,
        register_space: u32,
    }

    #[repr(C)]
    struct RootParameter {
        param_type: u32, // 2=CBV, 3=SRV, 4=UAV
        _pad0: u32,      // 8-byte alignment for C union containing pointer
        desc: RootDescriptor,
        _pad_union: u64, // C union size is 16 bytes (DescriptorTable)
        shader_visibility: u32,
        _pad_end: u32,
    }

    #[repr(C)]
    struct RootSignatureDesc {
        num_parameters: u32,
        parameters: *const RootParameter,
        num_static_samplers: u32,
        static_samplers: *const c_void,
        flags: u32,
    }

    #[repr(C)]
    struct ShaderBytecode {
        bytecode: *const c_void,
        bytecode_length: usize,
    }

    #[repr(C)]
    struct ComputePipelineStateDesc {
        root_signature: *mut c_void,
        cs: ShaderBytecode,
        node_mask: u32,
        cached_pso_blob: *const c_void,
        cached_pso_size: usize,
        flags: u32,
    }

    #[repr(C)]
    struct HeapProperties {
        ty: u32, // 1=DEFAULT, 2=UPLOAD, 3=READBACK
        cpu_page_property: u32,
        memory_pool_preference: u32,
        creation_node_mask: u32,
        visible_node_mask: u32,
    }

    #[repr(C)]
    struct ResourceDesc {
        dimension: u32, // 1=BUFFER
        alignment: u64,
        width: u64,
        height: u32,
        depth_or_array_size: u16,
        mip_levels: u16,
        format: u32,
        sample_count: u32,
        sample_quality: u32,
        layout: u32, // 1=ROW_MAJOR
        flags: u32,  // 4=ALLOW_UNORDERED_ACCESS
    }

    #[repr(C)]
    struct ResourceTransitionBarrier {
        resource: *mut c_void,
        subresource: u32,
        state_before: u32,
        state_after: u32,
    }

    #[repr(C)]
    struct ResourceBarrier {
        ty: u32, // 0=TRANSITION
        flags: u32,
        transition: ResourceTransitionBarrier,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateEventW(attrs: *const c_void, manual_reset: i32, initial_state: i32, name: *const u16) -> isize;
        fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
        fn CloseHandle(handle: isize) -> i32;
    }

    unsafe fn vtbl(obj: *mut c_void, slot: usize) -> *const c_void {
        let vt = *(obj as *const *const *const c_void);
        *vt.add(slot)
    }

    unsafe fn release(obj: *mut c_void) {
        if !obj.is_null() {
            let f: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vtbl(obj, 2));
            f(obj);
        }
    }

    fn buffer_desc(bytes: u64, flags: u32) -> ResourceDesc {
        ResourceDesc {
            dimension: 1,
            alignment: 0,
            width: bytes.max(256),
            height: 1,
            depth_or_array_size: 1,
            mip_levels: 1,
            format: 0,
            sample_count: 1,
            sample_quality: 0,
            layout: 1,
            flags,
        }
    }

    fn heap_props(ty: u32) -> HeapProperties {
        HeapProperties {
            ty,
            cpu_page_property: 0,
            memory_pool_preference: 0,
            creation_node_mask: 1,
            visible_node_mask: 1,
        }
    }

    unsafe fn create_buffer(dev: *mut c_void, heap_ty: u32, bytes: u64, flags: u32, init_state: u32) -> Result<*mut c_void, String> {
        let f: unsafe extern "system" fn(
            *mut c_void,
            *const HeapProperties,
            u32,
            *const ResourceDesc,
            u32,
            *const c_void,
            *const [u8; 16],
            *mut *mut c_void,
        ) -> i32 = std::mem::transmute(vtbl(dev, 27));
        let hp = heap_props(heap_ty);
        let desc = buffer_desc(bytes, flags);
        let mut res: *mut c_void = null_mut();
        let hr = f(dev, &hp, 0, &desc, init_state, null(), &GUID_ID3D12_RESOURCE, &mut res);
        if hr < 0 || res.is_null() {
            return Err(format!("CreateCommittedResource(heap={heap_ty}) failed: 0x{hr:08x}"));
        }
        Ok(res)
    }

    unsafe fn upload_words(res: *mut c_void, words: &[u32]) -> Result<(), String> {
        let map: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut u8) -> i32 =
            std::mem::transmute(vtbl(res, 8));
        let unmap: unsafe extern "system" fn(*mut c_void, u32, *const c_void) =
            std::mem::transmute(vtbl(res, 9));
        let mut ptr: *mut u8 = null_mut();
        let hr = map(res, 0, null(), &mut ptr);
        if hr < 0 || ptr.is_null() {
            return Err(format!("Map upload buffer failed: 0x{hr:08x}"));
        }
        if !words.is_empty() {
            std::ptr::copy_nonoverlapping(words.as_ptr() as *const u8, ptr, words.len() * 4);
        }
        unmap(res, 0, null());
        Ok(())
    }

    unsafe fn gpu_va(res: *mut c_void) -> u64 {
        let f: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(vtbl(res, 11));
        f(res)
    }

    /// Executes a compiled SM 6.6 DXIL compute shader on the hardware D3D12 GPU
    /// with `b0` (CBV), `t0` (`ByteAddressBuffer` input), and `u0` (`RWByteAddressBuffer` output),
    /// returning `output_words` `u32` values read back from GPU memory.
    pub fn run_compute_dxil(dxil: &[u8], input_words: &[u32], output_words: usize, groups_x: u32) -> Result<Vec<u32>, String> {
        if dxil.is_empty() || output_words == 0 {
            return Err("empty dxil or zero output_words".into());
        }
        let (p_create_dev, p_ser_rs) = unsafe {
            let _ = crate::host_d3d12();
            (
                crate::host_proc("D3D12CreateDevice"),
                crate::host_proc("D3D12SerializeRootSignature"),
            )
        };
        if p_create_dev == 0 || p_ser_rs == 0 {
            return Err("host d3d12.dll exports unavailable".into());
        }

        unsafe {
            let create_dev: unsafe extern "system" fn(*mut c_void, u32, *const [u8; 16], *mut *mut c_void) -> i32 =
                std::mem::transmute(p_create_dev);
            let mut dev: *mut c_void = null_mut();
            let hr = create_dev(null_mut(), 0xC000, &GUID_ID3D12_DEVICE, &mut dev);
            if hr < 0 || dev.is_null() {
                return Err(format!("D3D12CreateDevice failed: 0x{hr:08x}"));
            }

            let res = run_on_device(dev, p_ser_rs, dxil, input_words, output_words, groups_x);
            release(dev);
            res
        }
    }

    unsafe fn run_on_device(
        dev: *mut c_void,
        p_ser_rs: usize,
        dxil: &[u8],
        input_words: &[u32],
        output_words: usize,
        groups_x: u32,
    ) -> Result<Vec<u32>, String> {
        // Command queue
        let create_q: unsafe extern "system" fn(*mut c_void, *const CommandQueueDesc, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 8));
        let q_desc = CommandQueueDesc { ty: 0, priority: 0, flags: 0, node_mask: 0 };
        let mut queue: *mut c_void = null_mut();
        let hr = create_q(dev, &q_desc, &GUID_ID3D12_COMMAND_QUEUE, &mut queue);
        if hr < 0 || queue.is_null() {
            return Err(format!("CreateCommandQueue failed: 0x{hr:08x}"));
        }

        // Command allocator
        let create_alloc: unsafe extern "system" fn(*mut c_void, u32, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 9));
        let mut alloc: *mut c_void = null_mut();
        let hr = create_alloc(dev, 0, &GUID_ID3D12_COMMAND_ALLOCATOR, &mut alloc);
        if hr < 0 || alloc.is_null() {
            release(queue);
            return Err(format!("CreateCommandAllocator failed: 0x{hr:08x}"));
        }

        // Command list
        let create_cl: unsafe extern "system" fn(*mut c_void, u32, u32, *mut c_void, *mut c_void, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 12));
        let mut cl: *mut c_void = null_mut();
        let hr = create_cl(dev, 0, 0, alloc, null_mut(), &GUID_ID3D12_GRAPHICS_COMMAND_LIST, &mut cl);
        if hr < 0 || cl.is_null() {
            release(alloc);
            release(queue);
            return Err(format!("CreateCommandList failed: 0x{hr:08x}"));
        }

        // Root signature: b0 (CBV), t0 (SRV), u0 (UAV)
        let params = [
            RootParameter {
                param_type: 2, // CBV
                _pad0: 0,
                desc: RootDescriptor { shader_register: 0, register_space: 0 },
                _pad_union: 0,
                shader_visibility: 0,
                _pad_end: 0,
            },
            RootParameter {
                param_type: 3, // SRV
                _pad0: 0,
                desc: RootDescriptor { shader_register: 0, register_space: 0 },
                _pad_union: 0,
                shader_visibility: 0,
                _pad_end: 0,
            },
            RootParameter {
                param_type: 4, // UAV
                _pad0: 0,
                desc: RootDescriptor { shader_register: 0, register_space: 0 },
                _pad_union: 0,
                shader_visibility: 0,
                _pad_end: 0,
            },
        ];
        let rs_desc = RootSignatureDesc {
            num_parameters: params.len() as u32,
            parameters: params.as_ptr(),
            num_static_samplers: 0,
            static_samplers: null(),
            flags: 0,
        };
        let ser_rs: unsafe extern "system" fn(*const RootSignatureDesc, u32, *mut *mut c_void, *mut *mut c_void) -> i32 =
            std::mem::transmute(p_ser_rs);
        let mut rs_blob: *mut c_void = null_mut();
        let mut err_blob: *mut c_void = null_mut();
        let hr = ser_rs(&rs_desc, 1, &mut rs_blob, &mut err_blob);
        if hr < 0 || rs_blob.is_null() {
            release(err_blob);
            release(cl);
            release(alloc);
            release(queue);
            return Err(format!("D3D12SerializeRootSignature failed: 0x{hr:08x}"));
        }
        let blob_ptr: unsafe extern "system" fn(*mut c_void) -> *const c_void = std::mem::transmute(vtbl(rs_blob, 3));
        let blob_sz: unsafe extern "system" fn(*mut c_void) -> usize = std::mem::transmute(vtbl(rs_blob, 4));
        let create_rs: unsafe extern "system" fn(*mut c_void, u32, *const c_void, usize, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 16));
        let mut root_sig: *mut c_void = null_mut();
        let hr = create_rs(dev, 0, blob_ptr(rs_blob), blob_sz(rs_blob), &GUID_ID3D12_ROOT_SIGNATURE, &mut root_sig);
        release(rs_blob);
        release(err_blob);
        if hr < 0 || root_sig.is_null() {
            release(cl);
            release(alloc);
            release(queue);
            return Err(format!("CreateRootSignature failed: 0x{hr:08x}"));
        }

        // Compute PSO
        let pso_desc = ComputePipelineStateDesc {
            root_signature: root_sig,
            cs: ShaderBytecode { bytecode: dxil.as_ptr() as *const c_void, bytecode_length: dxil.len() },
            node_mask: 0,
            cached_pso_blob: null(),
            cached_pso_size: 0,
            flags: 0,
        };
        let create_pso: unsafe extern "system" fn(*mut c_void, *const ComputePipelineStateDesc, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 11));
        let mut pso: *mut c_void = null_mut();
        let hr = create_pso(dev, &pso_desc, &GUID_ID3D12_PIPELINE_STATE, &mut pso);
        if hr < 0 || pso.is_null() {
            release(root_sig);
            release(cl);
            release(alloc);
            release(queue);
            return Err(format!("CreateComputePipelineState failed: 0x{hr:08x}"));
        }

        // Buffers
        let cb_bytes = 1024u64;
        let in_bytes = ((input_words.len().max(1) * 4) as u64).max(256);
        let out_bytes = ((output_words * 4) as u64).max(256);
        let cb_buf = create_buffer(dev, 2, cb_bytes, 0, 0x1 | 0x2 | 0x40 | 0x80 | 0x200 | 0x800)?; // GENERIC_READ = 0xAC3? Use 0xAC3? Actually GENERIC_READ = 1|2|64|128|512|2048 = 0xAC3
        let in_buf = create_buffer(dev, 2, in_bytes, 0, 0xAC3)?;
        let out_buf = create_buffer(dev, 1, out_bytes, 4, 0x8)?; // UNORDERED_ACCESS
        let rb_buf = create_buffer(dev, 3, out_bytes, 0, 0x400)?; // COPY_DEST

        upload_words(cb_buf, &[0u32; 4])?;
        upload_words(in_buf, input_words)?;

        // Record
        let set_rs: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(vtbl(cl, 29));
        let set_pso: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(vtbl(cl, 25));
        let set_cbv: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(vtbl(cl, 37));
        let set_srv: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(vtbl(cl, 39));
        let set_uav: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(vtbl(cl, 41));
        let dispatch: unsafe extern "system" fn(*mut c_void, u32, u32, u32) = std::mem::transmute(vtbl(cl, 14));
        let barrier: unsafe extern "system" fn(*mut c_void, u32, *const ResourceBarrier) = std::mem::transmute(vtbl(cl, 26));
        let copy_res: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) = std::mem::transmute(vtbl(cl, 17));
        let close_cl: unsafe extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(vtbl(cl, 9));

        set_rs(cl, root_sig);
        set_pso(cl, pso);
        set_cbv(cl, 0, gpu_va(cb_buf));
        set_srv(cl, 1, gpu_va(in_buf));
        set_uav(cl, 2, gpu_va(out_buf));
        dispatch(cl, groups_x.max(1), 1, 1);

        let b = ResourceBarrier {
            ty: 0,
            flags: 0,
            transition: ResourceTransitionBarrier {
                resource: out_buf,
                subresource: 0xFFFF_FFFF,
                state_before: 0x8,   // UNORDERED_ACCESS
                state_after: 0x800,  // COPY_SOURCE
            },
        };
        barrier(cl, 1, &b);
        copy_res(cl, rb_buf, out_buf);
        let hr = close_cl(cl);
        if hr < 0 {
            release(rb_buf);
            release(out_buf);
            release(in_buf);
            release(cb_buf);
            release(pso);
            release(root_sig);
            release(cl);
            release(alloc);
            release(queue);
            return Err(format!("CommandList::Close failed: 0x{hr:08x}"));
        }

        // Execute & Fence wait
        let exec: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = std::mem::transmute(vtbl(queue, 10));
        let lists = [cl];
        exec(queue, 1, lists.as_ptr());

        let create_fence: unsafe extern "system" fn(*mut c_void, u64, u32, *const [u8; 16], *mut *mut c_void) -> i32 =
            std::mem::transmute(vtbl(dev, 36));
        let mut fence: *mut c_void = null_mut();
        let hr = create_fence(dev, 0, 0, &GUID_ID3D12_FENCE, &mut fence);
        if hr == S_OK && !fence.is_null() {
            let signal: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 = std::mem::transmute(vtbl(queue, 14));
            let completed: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(vtbl(fence, 8));
            let set_evt: unsafe extern "system" fn(*mut c_void, u64, isize) -> i32 = std::mem::transmute(vtbl(fence, 9));
            let _ = signal(queue, fence, 1);
            if completed(fence) < 1 {
                let evt = CreateEventW(null(), 0, 0, null());
                if evt != 0 {
                    let _ = set_evt(fence, 1, evt);
                    WaitForSingleObject(evt, 5000);
                    CloseHandle(evt);
                }
            }
            release(fence);
        }

        // Readback
        let map: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut u32) -> i32 =
            std::mem::transmute(vtbl(rb_buf, 8));
        let unmap: unsafe extern "system" fn(*mut c_void, u32, *const c_void) =
            std::mem::transmute(vtbl(rb_buf, 9));
        let mut rb_ptr: *mut u32 = null_mut();
        let hr = map(rb_buf, 0, null(), &mut rb_ptr);
        let mut out = vec![0u32; output_words];
        if hr >= 0 && !rb_ptr.is_null() {
            std::ptr::copy_nonoverlapping(rb_ptr, out.as_mut_ptr(), output_words);
            unmap(rb_buf, 0, null());
        }

        release(rb_buf);
        release(out_buf);
        release(in_buf);
        release(cb_buf);
        release(pso);
        release(root_sig);
        release(cl);
        release(alloc);
        release(queue);

        if hr < 0 {
            return Err(format!("Readback Map failed: 0x{hr:08x}"));
        }
        Ok(out)
    }
}

#[cfg(windows)]
pub use imp::run_compute_dxil;

#[cfg(not(windows))]
pub fn run_compute_dxil(_dxil: &[u8], _input_words: &[u32], _output_words: usize, _groups_x: u32) -> Result<Vec<u32>, String> {
    Err("D3D12 compute execution requires Windows".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::rdna2_hlsl::{lift_to_hlsl_strict, ShaderStage};

    #[test]
    fn executes_lifted_rdna2_compute_shader_on_hardware_gpu_and_matches_reference() {
        // Real RDNA2 compute program:
        //   v[1] = g_cbuf.Load(v[0] * 4): first compute v[2] = v[0] << 2
        //   v_lshlrev_b32 v2, 2, v0          (VOP2 op=0x1A, src0=130(2), vsrc1=0, vdst=2)
        //   buffer_load v1, v2, off=0        (MUBUF 0x38, op=0x00, vaddr=2, vdata=1)
        //   v_mul_f32 v3, 2.0, v1            (VOP2 op=0x08, src0=244(2.0), vsrc1=1, vdst=3)
        //   v_add_f32 v3, 1.0, v3            (VOP2 op=0x03, src0=242(1.0), vsrc1=3, vdst=3)
        //   buffer_store_dword v3, v2, off=0 (MUBUF 0x38, op=0x18, vaddr=2, vdata=3)
        //   s_endpgm
        let mut code = Vec::new();
        code.extend_from_slice(&((0x1Au32 << 25) | (2u32 << 17) | (0u32 << 9) | 130u32).to_le_bytes());
        code.extend_from_slice(&(0x38u32 << 26).to_le_bytes());
        code.extend_from_slice(&((1u32 << 8) | 2u32).to_le_bytes());
        code.extend_from_slice(&((0x08u32 << 25) | (3u32 << 17) | (1u32 << 9) | 244u32).to_le_bytes());
        code.extend_from_slice(&((0x03u32 << 25) | (3u32 << 17) | (3u32 << 9) | 242u32).to_le_bytes());
        code.extend_from_slice(&((0x38u32 << 26) | (0x18u32 << 18)).to_le_bytes());
        code.extend_from_slice(&((3u32 << 8) | 2u32).to_le_bytes());
        code.extend_from_slice(&0xBF81_0000u32.to_le_bytes());

        let (stage, hlsl, insns) = lift_to_hlsl_strict(&code, ShaderStage::Compute)
            .expect("strict RDNA2 lift must succeed with zero unknown opcodes");
        assert_eq!(stage, ShaderStage::Compute);
        assert_eq!(insns, 6);

        let dxil = crate::compile_hlsl_to_dxil(&hlsl, stage).expect("DXC SM 6.6 compile");
        let inputs: Vec<f32> = (0..16).map(|i| i as f32 * 0.5 - 2.0).collect();
        let input_words: Vec<u32> = inputs.iter().map(|f| f.to_bits()).collect();

        let gpu_words = run_compute_dxil(&dxil, &input_words, 16, 1)
            .expect("hardware D3D12 compute dispatch");
        for (i, &x) in inputs.iter().enumerate() {
            let expected = 2.0f32 * x + 1.0f32;
            let got = f32::from_bits(gpu_words[i]);
            assert!(
                (got - expected).abs() < 1e-6,
                "lane {i}: GPU computed {got} (0x{:08x}), expected {expected} (0x{:08x})",
                gpu_words[i],
                expected.to_bits()
            );
        }
    }

    #[test]
    fn executes_rdna2_loop_with_sopp_branch_on_hardware_gpu() {
        // Real RDNA2 loop program (4 iterations):
        //   0x00: s_mov_b32 s0, 0            (SOP1 op=0x03, sdst=0, ssrc0=128(0))
        //   0x04: v_mov_b32 v1, 1.0          (VOP1 op=0x01, vdst=1, src0=242(1.0))
        //   0x08: v_add_f32 v1, 2.0, v1      (VOP2 op=0x03, vdst=1, vsrc1=1, src0=244(2.0))
        //   0x0C: s_add_u32 s0, s0, 1        (SOP2 op=0x00, sdst=0, ssrc0=0, ssrc1=129(1))
        //   0x10: s_cmp_lt_u32 s0, 4         (SOPC op=0x0A, ssrc0=0, ssrc1=132(4))
        //   0x14: s_cbranch_scc1 -4          (SOPP op=0x05, imm=-4 -> 0x14 + 4 - 16 = 0x08)
        //   0x18: v_lshlrev_b32 v2, 2, v0    (VOP2 op=0x1A, vdst=2, vsrc1=0, src0=130(2))
        //   0x1C: buffer_store_dword v1, v2  (MUBUF 0x38 op=0x18, vaddr=2, vdata=1)
        //   0x24: s_endpgm
        let mut code = Vec::new();
        code.extend_from_slice(&((0x17Du32 << 23) | (0x03u32 << 8) | 128u32).to_le_bytes());
        code.extend_from_slice(&((0x3Fu32 << 25) | (1u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes());
        code.extend_from_slice(&((0x03u32 << 25) | (1u32 << 17) | (1u32 << 9) | 244u32).to_le_bytes());
        code.extend_from_slice(&((0x80u32 << 24) | (129u32 << 8) | 0u32).to_le_bytes());
        code.extend_from_slice(&((0x17Eu32 << 23) | (0x0Au32 << 16) | (132u32 << 8) | 0u32).to_le_bytes());
        code.extend_from_slice(&(0xBF80_0000u32 | (0x05u32 << 16) | ((-4i16 as u16) as u32)).to_le_bytes());
        code.extend_from_slice(&((0x1Au32 << 25) | (2u32 << 17) | (0u32 << 9) | 130u32).to_le_bytes());
        code.extend_from_slice(&((0x38u32 << 26) | (0x18u32 << 18)).to_le_bytes());
        code.extend_from_slice(&((1u32 << 8) | 2u32).to_le_bytes());
        code.extend_from_slice(&0xBF81_0000u32.to_le_bytes());

        let (stage, hlsl, _) = lift_to_hlsl_strict(&code, ShaderStage::Compute)
            .expect("strict loop lift");
        let dxil = crate::compile_hlsl_to_dxil(&hlsl, stage).expect("DXC SM 6.6 compile");
        let gpu_words = run_compute_dxil(&dxil, &[0u32; 8], 8, 1).expect("GPU loop dispatch");
        // 1.0 + 4 * 2.0 = 9.0
        for (i, &w) in gpu_words.iter().enumerate() {
            let got = f32::from_bits(w);
            assert!((got - 9.0).abs() < 1e-6, "lane {i}: got {got}, expected 9.0");
        }
    }
}
