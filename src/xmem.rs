use std::collections::HashMap;

#[derive(Default)]
pub struct PhysicalPool {
    allocations: HashMap<usize, usize>,
    total_bytes: usize,
}

impl PhysicalPool {
    pub fn alloc(&mut self, size: usize) -> Result<usize, String> {
        let aligned = (size.max(4096) + 0xFFF) & !0xFFF;
        #[cfg(windows)]
        {
            extern "system" {
                fn VirtualAlloc(addr: usize, size: usize, alloc_type: u32, protect: u32) -> usize;
            }
            const MEM_COMMIT_RESERVE: u32 = 0x3000;
            const PAGE_READWRITE: u32 = 0x04;
            let ptr = unsafe { VirtualAlloc(0, aligned, MEM_COMMIT_RESERVE, PAGE_READWRITE) };
            if ptr == 0 {
                return Err("VirtualAlloc failed".into());
            }
            self.allocations.insert(ptr, aligned);
            self.total_bytes += aligned;
            Ok(ptr)
        }
        #[cfg(not(windows))]
        {
            let id = self.allocations.len() + 1;
            self.allocations.insert(id * 0x10000, aligned);
            self.total_bytes += aligned;
            Ok(id * 0x10000)
        }
    }

    pub fn free(&mut self, ptr: usize) -> bool {
        let Some(sz) = self.allocations.remove(&ptr) else {
            return false;
        };
        self.total_bytes = self.total_bytes.saturating_sub(sz);
        #[cfg(windows)]
        {
            extern "system" {
                fn VirtualFree(addr: usize, size: usize, free_type: u32) -> i32;
            }
            const MEM_RELEASE: u32 = 0x8000;
            unsafe { VirtualFree(ptr, 0, MEM_RELEASE) };
        }
        true
    }

    pub fn active_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn active_count(&self) -> usize {
        self.allocations.len()
    }
}

impl Drop for PhysicalPool {
    fn drop(&mut self) {
        let ptrs: Vec<usize> = self.allocations.keys().copied().collect();
        for p in ptrs {
            self.free(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_and_frees_physical_pool_blocks() {
        let mut pool = PhysicalPool::default();
        let p = pool.alloc(65536).unwrap();
        assert_eq!(pool.active_count(), 1);
        assert_eq!(pool.active_bytes(), 65536);
        assert!(pool.free(p));
        assert_eq!(pool.active_count(), 0);
        assert_eq!(pool.active_bytes(), 0);
    }
}
