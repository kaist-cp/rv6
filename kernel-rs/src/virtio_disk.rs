use crate::{
    buf::Buf,
    libc,
    printf::panic,
    proc::{cpu, sleep, wakeup},
    riscv::{PGSHIFT, PGSIZE},
    spinlock::{acquire, initlock, release, Spinlock},
    string::memset,
    vm::kvmpa,
};
use core::ptr;
/// driver for qemu's virtio disk device.
/// uses qemu's mmio interface to virtio.
/// qemu presents a "legacy" virtio interface.
///
/// qemu ... -drive file=fs.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
///
/// the address of virtio mmio register r.
#[derive(Copy, Clone)]
#[repr(C, align(4096))]
pub struct Disk(pub disk_Inner);
#[derive(Copy, Clone)]
#[repr(C)]
pub struct disk_Inner {
    pub pages: [libc::c_char; 8192],
    pub desc: *mut VRingDesc,
    pub avail: *mut u16,
    pub used: *mut UsedArea,
    pub free: [libc::c_char; 8],
    pub used_idx: u16,
    pub info: [C2RustUnnamed; 8],
    pub vdisk_lock: Spinlock,
}
#[allow(dead_code, non_upper_case_globals)]
const disk_PADDING: usize = ::core::mem::size_of::<Disk>() - ::core::mem::size_of::<disk_Inner>();
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed {
    pub b: *mut Buf,
    pub status: libc::c_char,
}
/// write the disk
#[derive(Copy, Clone)]
#[repr(C)]
pub struct UsedArea {
    pub flags: u16,
    pub id: u16,
    pub elems: [VRingUsedElem; 8],
}
/// device writes (vs read)
#[derive(Copy, Clone)]
#[repr(C)]
pub struct VRingUsedElem {
    pub id: u32,
    pub len: u32,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct VRingDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct virtio_blk_outhdr {
    pub type_0: u32,
    pub reserved: u32,
    pub sector: u64,
}
// Physical memory layout
// qemu -machine virt is set up like this,
// based on qemu's hw/riscv/virt.c:
//
// 00001000 -- boot ROM, provided by qemu
// 02000000 -- CLINT
// 0C000000 -- PLIC
// 10000000 -- uart0
// 10001000 -- virtio disk
// 80000000 -- boot ROM jumps here in machine mode
//             -kernel loads the kernel here
// unused RAM after 80000000.
// the kernel uses physical memory thus:
// 80000000 -- entry.S, then kernel text and data
// end -- start of kernel page allocation area
// PHYSTOP -- end RAM used by the kernel
// qemu puts UART registers here in physical memory.
// virtio mmio interface
pub const VIRTIO0: i32 = 0x10001000;
// On-disk file system format.
// Both the kernel and user programs use this header file.
// root i-number
pub const BSIZE: i32 = 1024;
//
// virtio device definitions.
// for both the mmio interface, and virtio descriptors.
// only tested with qemu.
// this is the "legacy" virtio interface.
//
// the virtio spec:
// https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.pdf
//
// virtio mmio control registers, mapped starting at 0x10001000.
// from qemu virtio_mmio.h
// 0x74726976
// version; 1 is legacy
// device type; 1 is net, 2 is disk
// 0x554d4551
// page size for PFN, write-only
// select queue, write-only
// max size of current queue, read-only
// size of current queue, write-only
// used ring alignment, write-only
// physical page number for queue, read/write
// ready bit
// write-only
// read-only
// write-only
// read/write
// status register bits, from qemu virtio_config.h
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: i32 = 1;
pub const VIRTIO_CONFIG_S_DRIVER: i32 = 2;
pub const VIRTIO_CONFIG_S_DRIVER_OK: i32 = 4;
pub const VIRTIO_CONFIG_S_FEATURES_OK: i32 = 8;
// device feature bits
pub const VIRTIO_BLK_F_RO: i32 = 5;
/* Disk is read-only */
pub const VIRTIO_BLK_F_SCSI: i32 = 7;
/* Supports scsi command passthru */
pub const VIRTIO_BLK_F_CONFIG_WCE: i32 = 11;
/* Writeback mode available in config */
pub const VIRTIO_BLK_F_MQ: i32 = 12;
/* support more than one vq */
pub const VIRTIO_F_ANY_LAYOUT: i32 = 27;
pub const VIRTIO_RING_F_INDIRECT_DESC: i32 = 28;
pub const VIRTIO_RING_F_EVENT_IDX: i32 = 29;
// this many virtio descriptors.
// must be a power of two.
pub const NUM: i32 = 8;
pub const VRING_DESC_F_NEXT: i32 = 1;
// chained with another descriptor
pub const VRING_DESC_F_WRITE: i32 = 2;
// for disk ops
pub const VIRTIO_BLK_T_IN: i32 = 0;
// read the disk
pub const VIRTIO_BLK_T_OUT: i32 = 1;
static mut disk: Disk = Disk(disk_Inner {
    pages: [0; 8192],
    desc: 0 as *const VRingDesc as *mut VRingDesc,
    avail: 0 as *const u16 as *mut u16,
    used: 0 as *const UsedArea as *mut UsedArea,
    free: [0; 8],
    used_idx: 0,
    info: [C2RustUnnamed {
        b: 0 as *const Buf as *mut Buf,
        status: 0,
    }; 8],
    vdisk_lock: Spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
});
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_init() {
    let mut status: u32 = 0 as u32;
    initlock(
        &mut disk.0.vdisk_lock,
        b"virtio_disk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    if *((VIRTIO0 + 0 as i32) as *mut u32) != 0x74726976 as u32
        || *((VIRTIO0 + 0x4 as i32) as *mut u32) != 1 as u32
        || *((VIRTIO0 + 0x8 as i32) as *mut u32) != 2 as u32
        || *((VIRTIO0 + 0xc as i32) as *mut u32) != 0x554d4551 as u32
    {
        panic(
            b"could not find virtio disk\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    status |= VIRTIO_CONFIG_S_ACKNOWLEDGE as u32;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as i32) as *mut u32, status);
    status |= VIRTIO_CONFIG_S_DRIVER as u32;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as i32) as *mut u32, status);
    // negotiate features
    let mut features: u64 = *((VIRTIO0 + 0x10 as i32) as *mut u32) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_RO) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_SCSI) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_CONFIG_WCE) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_MQ) as u64;
    features &= !((1 as i32) << VIRTIO_F_ANY_LAYOUT) as u64;
    features &= !((1 as i32) << VIRTIO_RING_F_EVENT_IDX) as u64;
    features &= !((1 as i32) << VIRTIO_RING_F_INDIRECT_DESC) as u64;
    ::core::ptr::write_volatile((VIRTIO0 + 0x20 as i32) as *mut u32, features as u32);
    // tell device that feature negotiation is complete.
    status |= VIRTIO_CONFIG_S_FEATURES_OK as u32;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as i32) as *mut u32, status);
    // tell device we're completely ready.
    status |= VIRTIO_CONFIG_S_DRIVER_OK as u32;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as i32) as *mut u32, status);
    ::core::ptr::write_volatile((VIRTIO0 + 0x28 as i32) as *mut u32, PGSIZE as u32);
    // initialize queue 0.
    ::core::ptr::write_volatile((VIRTIO0 + 0x30 as i32) as *mut u32, 0);
    let mut max: u32 = *((VIRTIO0 + 0x34 as i32) as *mut u32);
    if max == 0 as u32 {
        panic(
            b"virtio disk has no queue 0\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    if max < NUM as u32 {
        panic(
            b"virtio disk max queue too short\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    ::core::ptr::write_volatile((VIRTIO0 + 0x38 as i32) as *mut u32, NUM as u32);
    memset(
        disk.0.pages.as_mut_ptr() as *mut libc::c_void,
        0 as i32,
        ::core::mem::size_of::<[libc::c_char; 8192]>() as u64 as u32,
    );
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x40 as i32) as *mut u32,
        (disk.0.pages.as_mut_ptr() as u64 >> PGSHIFT) as u32,
    );
    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * u16, then num * u16
    // used = pages + 4096 -- 2 * u16, then num * vRingUsedElem
    disk.0.desc = disk.0.pages.as_mut_ptr() as *mut VRingDesc;
    disk.0.avail = (disk.0.desc as *mut libc::c_char)
        .offset((NUM as u64).wrapping_mul(::core::mem::size_of::<VRingDesc>() as u64) as isize)
        as *mut u16;
    disk.0.used = disk.0.pages.as_mut_ptr().offset(PGSIZE as isize) as *mut UsedArea;
    let mut i: i32 = 0;
    while i < NUM {
        disk.0.free[i as usize] = 1 as i32 as libc::c_char;
        i += 1
    }
    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}
/// find a free descriptor, mark it non-free, return its index.
unsafe extern "C" fn alloc_desc() -> i32 {
    let mut i: i32 = 0;
    while i < NUM {
        if disk.0.free[i as usize] != 0 {
            disk.0.free[i as usize] = 0 as i32 as libc::c_char;
            return i;
        }
        i += 1
    }
    -1
}
/// mark a descriptor as free.
unsafe extern "C" fn free_desc(mut i: i32) {
    if i >= NUM {
        panic(b"virtio_disk_intr 1\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if disk.0.free[i as usize] != 0 {
        panic(b"virtio_disk_intr 2\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*disk.0.desc.offset(i as isize)).addr = 0 as i32 as u64;
    disk.0.free[i as usize] = 1 as i32 as libc::c_char;
    wakeup(
        &mut *disk.0.free.as_mut_ptr().offset(0 as i32 as isize) as *mut libc::c_char
            as *mut libc::c_void,
    );
}
/// free a chain of descriptors.
unsafe extern "C" fn free_chain(mut i: i32) {
    loop {
        free_desc(i);
        if (*disk.0.desc.offset(i as isize)).flags as i32 & VRING_DESC_F_NEXT == 0 {
            break;
        }
        i = (*disk.0.desc.offset(i as isize)).next as i32
    }
}
unsafe extern "C" fn alloc3_desc(mut idx: *mut i32) -> i32 {
    let mut i: i32 = 0;
    while i < 3 as i32 {
        *idx.offset(i as isize) = alloc_desc();
        if *idx.offset(i as isize) < 0 as i32 {
            let mut j: i32 = 0;
            while j < i {
                free_desc(*idx.offset(j as isize));
                j += 1
            }
            return -(1 as i32);
        }
        i += 1
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_rw(mut b: *mut Buf, mut write: i32) {
    let mut sector: u64 = (*b).blockno.wrapping_mul((BSIZE / 512 as i32) as u32) as u64;
    acquire(&mut disk.0.vdisk_lock);
    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.
    // allocate the three descriptors.
    let mut idx: [i32; 3] = [0; 3];
    while alloc3_desc(idx.as_mut_ptr()) != 0 as i32 {
        sleep(
            &mut *disk.0.free.as_mut_ptr().offset(0 as i32 as isize) as *mut libc::c_char
                as *mut libc::c_void,
            &mut disk.0.vdisk_lock,
        );
    }
    // format the three descriptors.
    // qemu's virtio-blk.c reads them.
    let mut buf0: virtio_blk_outhdr = virtio_blk_outhdr {
        type_0: 0,
        reserved: 0,
        sector: 0,
    }; // read the disk
    if write != 0 {
        buf0.type_0 = VIRTIO_BLK_T_OUT as u32
    } else {
        buf0.type_0 = VIRTIO_BLK_T_IN as u32
    } // write the disk
    buf0.reserved = 0 as u32;
    buf0.sector = sector;
    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    (*disk.0.desc.offset(idx[0 as i32 as usize] as isize)).addr =
        kvmpa(&mut buf0 as *mut virtio_blk_outhdr as u64); // device writes b->data
    (*disk.0.desc.offset(idx[0 as i32 as usize] as isize)).len =
        ::core::mem::size_of::<virtio_blk_outhdr>() as u64 as u32; // device reads b->data
    (*disk.0.desc.offset(idx[0 as i32 as usize] as isize)).flags = VRING_DESC_F_NEXT as u16; // device writes the status
    (*disk.0.desc.offset(idx[0 as i32 as usize] as isize)).next = idx[1 as i32 as usize] as u16;
    (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).addr = (*b).data.as_mut_ptr() as u64;
    (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).len = BSIZE as u32;
    if write != 0 {
        (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).flags = 0 as i32 as u16
    } else {
        (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).flags = VRING_DESC_F_WRITE as u16
    }
    let fresh0 = &mut (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).flags;
    *fresh0 = (*fresh0 as i32 | VRING_DESC_F_NEXT) as u16;
    (*disk.0.desc.offset(idx[1 as i32 as usize] as isize)).next = idx[2 as i32 as usize] as u16;
    disk.0.info[idx[0 as i32 as usize] as usize].status = 0 as i32 as libc::c_char;
    (*disk.0.desc.offset(idx[2 as i32 as usize] as isize)).addr = &mut (*disk
        .0
        .info
        .as_mut_ptr()
        .offset(*idx.as_mut_ptr().offset(0 as i32 as isize) as isize))
    .status as *mut libc::c_char
        as u64;
    (*disk.0.desc.offset(idx[2 as i32 as usize] as isize)).len = 1 as u32;
    (*disk.0.desc.offset(idx[2 as i32 as usize] as isize)).flags = VRING_DESC_F_WRITE as u16;
    (*disk.0.desc.offset(idx[2 as i32 as usize] as isize)).next = 0 as i32 as u16;
    // record struct Buf for virtio_disk_intr().
    (*b).disk = 1 as i32;
    disk.0.info[idx[0 as i32 as usize] as usize].b = b;
    // avail[0] is flags
    // avail[1] tells the device how far to look in avail[2...].
    // avail[2...] are desc[] indices the device should process.
    // we only tell device the first index in our chain of descriptors.
    *disk
        .0
        .avail
        .offset((2 as i32 + *disk.0.avail.offset(1 as i32 as isize) as i32 % NUM) as isize) =
        idx[0 as i32 as usize] as u16; // value is queue number
    ::core::intrinsics::atomic_fence();
    *disk.0.avail.offset(1 as i32 as isize) =
        (*disk.0.avail.offset(1 as i32 as isize) as i32 + 1 as i32) as u16;
    ::core::ptr::write_volatile((VIRTIO0 + 0x50 as i32) as *mut u32, 0);
    // Wait for virtio_disk_intr() to say request has finished.
    while (*b).disk == 1 as i32 {
        sleep(b as *mut libc::c_void, &mut disk.0.vdisk_lock); // disk is done with Buf
    }
    disk.0.info[idx[0 as i32 as usize] as usize].b = ptr::null_mut();
    free_chain(idx[0 as i32 as usize]);
    release(&mut disk.0.vdisk_lock);
}
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_intr() {
    acquire(&mut disk.0.vdisk_lock);
    while disk.0.used_idx as i32 % NUM != (*disk.0.used).id as i32 % NUM {
        let mut id: i32 = (*disk.0.used).elems[disk.0.used_idx as usize].id as i32;
        if disk.0.info[id as usize].status as i32 != 0 as i32 {
            panic(
                b"virtio_disk_intr status\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        (*disk.0.info[id as usize].b).disk = 0 as i32;
        wakeup(disk.0.info[id as usize].b as *mut libc::c_void);
        disk.0.used_idx = ((disk.0.used_idx as i32 + 1 as i32) % NUM) as u16
    }
    release(&mut disk.0.vdisk_lock);
}
