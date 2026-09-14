use core::alloc::{GlobalAlloc, Layout};

unsafe extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_free_page(addr: *mut u8);
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_pages(addr: *mut u8, count: u64);
    fn kmalloc(size: u64) -> *mut u8;
    fn kfree(ptr: *mut u8);
}

#[cfg(target_arch = "aarch64")]
const KERNEL_BASE: u64 = 0u64;

#[cfg(not(target_arch = "aarch64"))]
const KERNEL_BASE: u64 = 0xFFFF800000000000u64;

pub struct KernelAllocator;

const PAGE_THRESHOLD: usize = 2048;

const TAG_KMALLOC: u64 = 0xA115_4B4D_414C_4C01;
const TAG_PMM_PAGE: u64 = 0xA115_504D_4D50_4702;
const TAG_PMM_PAGES: u64 = 0xA115_504D_4D50_4703;
const TAG_SIZE: usize = core::mem::size_of::<u64>();

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            let size = layout.size();
            let tag_offset = if size <= PAGE_THRESHOLD { TAG_SIZE } else { 0 };

            if size <= PAGE_THRESHOLD {
                let kmalloc_ptr = kmalloc((size + tag_offset) as u64);
                if !kmalloc_ptr.is_null() {
                    let raw = kmalloc_ptr as *mut u8;
                    let tag_ptr = raw as *mut u64;
                    *tag_ptr = TAG_KMALLOC;
                    return raw.add(TAG_SIZE);
                }
            }

            let pages_needed = (size + tag_offset).div_ceil(4096);
            let tag: u64 = if pages_needed == 1 {
                TAG_PMM_PAGE
            } else {
                TAG_PMM_PAGES
            };

            if pages_needed == 1 {
                let phys = pmm_alloc_page() as u64;
                let virt = (phys + KERNEL_BASE) as *mut u8;
                if tag_offset > 0 {
                    let tag_ptr = virt as *mut u64;
                    *tag_ptr = tag;
                    virt.add(TAG_SIZE)
                } else {
                    virt
                }
            } else {
                let phys = pmm_alloc_pages(pages_needed as u64) as u64;
                let virt = (phys + KERNEL_BASE) as *mut u8;
                if tag_offset > 0 {
                    let tag_ptr = virt as *mut u64;
                    *tag_ptr = tag;
                    virt.add(TAG_SIZE)
                } else {
                    virt
                }
            }
        }
    }

    #[expect(
        clippy::match_same_arms,
        reason = "TAG_KMALLOC 与 wildcard body 相同是已知安全; 显式列举便于未来扩展"
    )]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            if ptr.is_null() {
                return;
            }

            let size = layout.size();
            if size <= PAGE_THRESHOLD {
                let raw = ptr.sub(TAG_SIZE);
                let tag = *(raw as *const u64);
                match tag {
                    TAG_KMALLOC => {
                        kfree(raw as *mut u8);
                    }
                    TAG_PMM_PAGE => {
                        let phys_addr = (raw as u64) - KERNEL_BASE;
                        pmm_free_page(phys_addr as *mut u8);
                    }
                    TAG_PMM_PAGES => {
                        let phys_addr = (raw as u64) - KERNEL_BASE;
                        let pages_needed = (size + TAG_SIZE).div_ceil(4096) as u64;
                        pmm_free_pages(phys_addr as *mut u8, pages_needed);
                    }
                    _ => {
                        kfree(raw as *mut u8);
                    }
                }
            } else {
                let pages = size.div_ceil(4096) as u64;
                let phys_addr = (ptr as u64) - KERNEL_BASE;
                if pages <= 1 {
                    pmm_free_page(phys_addr as *mut u8);
                } else {
                    pmm_free_pages(phys_addr as *mut u8, pages);
                }
            }
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
