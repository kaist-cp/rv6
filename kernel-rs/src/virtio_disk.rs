use crate::libc;
use core::ptr;
extern "C" {
    pub type cpu;
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut spinlock);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: libc::c_int, _: uint) -> *mut libc::c_void;
    #[no_mangle]
    fn kvmpa(_: uint64) -> uint64;
}
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
pub type uint16 = libc::c_ushort;
pub type uint32 = libc::c_uint;
pub type uint64 = libc::c_ulong;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct buf {
    pub valid: libc::c_int,
    pub disk: libc::c_int,
    pub dev: uint,
    pub blockno: uint,
    pub lock: sleeplock,
    pub refcnt: uint,
    pub prev: *mut buf,
    pub next: *mut buf,
    pub qnext: *mut buf,
    pub data: [uchar; 1024],
}
// Long-term locks for processes
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sleeplock {
    pub locked: uint,
    pub lk: spinlock,
    pub name: *mut libc::c_char,
    pub pid: libc::c_int,
}
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
//
// driver for qemu's virtio disk device.
// uses qemu's mmio interface to virtio.
// qemu presents a "legacy" virtio interface.
//
// qemu ... -drive file=fs.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
//
// the address of virtio mmio register r.
#[derive(Copy, Clone)]
#[repr(C, align(4096))]
pub struct disk(pub disk_Inner);
#[derive(Copy, Clone)]
#[repr(C)]
pub struct disk_Inner {
    pub pages: [libc::c_char; 8192],
    pub desc: *mut VRingDesc,
    pub avail: *mut uint16,
    pub used: *mut UsedArea,
    pub free: [libc::c_char; 8],
    pub used_idx: uint16,
    pub info: [C2RustUnnamed; 8],
    pub vdisk_lock: spinlock,
}
#[allow(dead_code, non_upper_case_globals)]
const disk_PADDING: usize = ::core::mem::size_of::<disk>() - ::core::mem::size_of::<disk_Inner>();
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed {
    pub b: *mut buf,
    pub status: libc::c_char,
}
// write the disk
#[derive(Copy, Clone)]
#[repr(C)]
pub struct UsedArea {
    pub flags: uint16,
    pub id: uint16,
    pub elems: [VRingUsedElem; 8],
}
// device writes (vs read)
#[derive(Copy, Clone)]
#[repr(C)]
pub struct VRingUsedElem {
    pub id: uint32,
    pub len: uint32,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct VRingDesc {
    pub addr: uint64,
    pub len: uint32,
    pub flags: uint16,
    pub next: uint16,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct virtio_blk_outhdr {
    pub type_0: uint32,
    pub reserved: uint32,
    pub sector: uint64,
}
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// bytes per page
pub const PGSHIFT: libc::c_int = 12 as libc::c_int;
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
pub const VIRTIO0: libc::c_int = 0x10001000 as libc::c_int;
// On-disk file system format.
// Both the kernel and user programs use this header file.
// root i-number
pub const BSIZE: libc::c_int = 1024 as libc::c_int;
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
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: libc::c_int = 1 as libc::c_int;
pub const VIRTIO_CONFIG_S_DRIVER: libc::c_int = 2 as libc::c_int;
pub const VIRTIO_CONFIG_S_DRIVER_OK: libc::c_int = 4 as libc::c_int;
pub const VIRTIO_CONFIG_S_FEATURES_OK: libc::c_int = 8 as libc::c_int;
// device feature bits
pub const VIRTIO_BLK_F_RO: libc::c_int = 5 as libc::c_int;
/* Disk is read-only */
pub const VIRTIO_BLK_F_SCSI: libc::c_int = 7 as libc::c_int;
/* Supports scsi command passthru */
pub const VIRTIO_BLK_F_CONFIG_WCE: libc::c_int = 11 as libc::c_int;
/* Writeback mode available in config */
pub const VIRTIO_BLK_F_MQ: libc::c_int = 12 as libc::c_int;
/* support more than one vq */
pub const VIRTIO_F_ANY_LAYOUT: libc::c_int = 27 as libc::c_int;
pub const VIRTIO_RING_F_INDIRECT_DESC: libc::c_int = 28 as libc::c_int;
pub const VIRTIO_RING_F_EVENT_IDX: libc::c_int = 29 as libc::c_int;
// this many virtio descriptors.
// must be a power of two.
pub const NUM: libc::c_int = 8 as libc::c_int;
pub const VRING_DESC_F_NEXT: libc::c_int = 1 as libc::c_int;
// chained with another descriptor
pub const VRING_DESC_F_WRITE: libc::c_int = 2 as libc::c_int;
// for disk ops
pub const VIRTIO_BLK_T_IN: libc::c_int = 0 as libc::c_int;
// read the disk
pub const VIRTIO_BLK_T_OUT: libc::c_int = 1 as libc::c_int;
static mut disk_global: disk = disk(disk_Inner {
    pages: [0; 8192],
    desc: 0 as *const VRingDesc as *mut VRingDesc,
    avail: 0 as *const uint16 as *mut uint16,
    used: 0 as *const UsedArea as *mut UsedArea,
    free: [0; 8],
    used_idx: 0,
    info: [C2RustUnnamed {
        b: 0 as *const buf as *mut buf,
        status: 0,
    }; 8],
    vdisk_lock: spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
});
// virtio_disk.c
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_init() {
    let mut status: uint32 = 0 as libc::c_int as uint32;
    initlock(
        &mut disk_global.0.vdisk_lock,
        b"virtio_disk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    if *((VIRTIO0 + 0 as libc::c_int) as *mut uint32) != 0x74726976 as libc::c_int as libc::c_uint
        || *((VIRTIO0 + 0x4 as libc::c_int) as *mut uint32) != 1 as libc::c_int as libc::c_uint
        || *((VIRTIO0 + 0x8 as libc::c_int) as *mut uint32) != 2 as libc::c_int as libc::c_uint
        || *((VIRTIO0 + 0xc as libc::c_int) as *mut uint32)
            != 0x554d4551 as libc::c_int as libc::c_uint
    {
        panic(
            b"could not find virtio disk\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    status |= VIRTIO_CONFIG_S_ACKNOWLEDGE as libc::c_uint;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as libc::c_int) as *mut uint32, status);
    status |= VIRTIO_CONFIG_S_DRIVER as libc::c_uint;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as libc::c_int) as *mut uint32, status);
    // negotiate features
    let mut features: uint64 = *((VIRTIO0 + 0x10 as libc::c_int) as *mut uint32) as uint64;
    features &= !((1 as libc::c_int) << VIRTIO_BLK_F_RO) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_BLK_F_SCSI) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_BLK_F_CONFIG_WCE) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_BLK_F_MQ) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_F_ANY_LAYOUT) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_RING_F_EVENT_IDX) as libc::c_ulong;
    features &= !((1 as libc::c_int) << VIRTIO_RING_F_INDIRECT_DESC) as libc::c_ulong;
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x20 as libc::c_int) as *mut uint32,
        features as uint32,
    );
    // tell device that feature negotiation is complete.
    status |= VIRTIO_CONFIG_S_FEATURES_OK as libc::c_uint;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as libc::c_int) as *mut uint32, status);
    // tell device we're completely ready.
    status |= VIRTIO_CONFIG_S_DRIVER_OK as libc::c_uint;
    ::core::ptr::write_volatile((VIRTIO0 + 0x70 as libc::c_int) as *mut uint32, status);
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x28 as libc::c_int) as *mut uint32,
        PGSIZE as uint32,
    );
    // initialize queue 0.
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x30 as libc::c_int) as *mut uint32,
        0 as libc::c_int as uint32,
    );
    let mut max: uint32 = *((VIRTIO0 + 0x34 as libc::c_int) as *mut uint32);
    if max == 0 as libc::c_int as libc::c_uint {
        panic(
            b"virtio disk has no queue 0\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    if max < NUM as libc::c_uint {
        panic(
            b"virtio disk max queue too short\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x38 as libc::c_int) as *mut uint32,
        NUM as uint32,
    );
    memset(
        disk_global.0.pages.as_mut_ptr() as *mut libc::c_void,
        0 as libc::c_int,
        ::core::mem::size_of::<[libc::c_char; 8192]>() as libc::c_ulong as uint,
    );
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x40 as libc::c_int) as *mut uint32,
        (disk_global.0.pages.as_mut_ptr() as uint64 >> PGSHIFT) as uint32,
    );
    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * uint16, then num * uint16
    // used = pages + 4096 -- 2 * uint16, then num * vRingUsedElem
    disk_global.0.desc = disk_global.0.pages.as_mut_ptr() as *mut VRingDesc;
    disk_global.0.avail = (disk_global.0.desc as *mut libc::c_char).offset(
        (NUM as libc::c_ulong).wrapping_mul(::core::mem::size_of::<VRingDesc>() as libc::c_ulong)
            as isize,
    ) as *mut uint16;
    disk_global.0.used = disk_global.0.pages.as_mut_ptr().offset(PGSIZE as isize) as *mut UsedArea;
    let mut i: libc::c_int = 0 as libc::c_int;
    while i < NUM {
        disk_global.0.free[i as usize] = 1 as libc::c_int as libc::c_char;
        i += 1
    }
    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}
// find a free descriptor, mark it non-free, return its index.
unsafe extern "C" fn alloc_desc() -> libc::c_int {
    let mut i: libc::c_int = 0 as libc::c_int;
    while i < NUM {
        if disk_global.0.free[i as usize] != 0 {
            disk_global.0.free[i as usize] = 0 as libc::c_int as libc::c_char;
            return i;
        }
        i += 1
    }
    -(1 as libc::c_int)
}
// mark a descriptor as free.
unsafe extern "C" fn free_desc(mut i: libc::c_int) {
    if i >= NUM {
        panic(b"virtio_disk_intr 1\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if disk_global.0.free[i as usize] != 0 {
        panic(b"virtio_disk_intr 2\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*disk_global.0.desc.offset(i as isize)).addr = 0 as libc::c_int as uint64;
    disk_global.0.free[i as usize] = 1 as libc::c_int as libc::c_char;
    wakeup(
        &mut *disk_global
            .0
            .free
            .as_mut_ptr()
            .offset(0 as libc::c_int as isize) as *mut libc::c_char as *mut libc::c_void,
    );
}
// free a chain of descriptors.
unsafe extern "C" fn free_chain(mut i: libc::c_int) {
    loop {
        free_desc(i);
        if !((*disk_global.0.desc.offset(i as isize)).flags as libc::c_int & VRING_DESC_F_NEXT != 0)
        {
            break;
        }
        i = (*disk_global.0.desc.offset(i as isize)).next as libc::c_int
    }
}
unsafe extern "C" fn alloc3_desc(mut idx: *mut libc::c_int) -> libc::c_int {
    let mut i: libc::c_int = 0 as libc::c_int;
    while i < 3 as libc::c_int {
        *idx.offset(i as isize) = alloc_desc();
        if *idx.offset(i as isize) < 0 as libc::c_int {
            let mut j: libc::c_int = 0 as libc::c_int;
            while j < i {
                free_desc(*idx.offset(j as isize));
                j += 1
            }
            return -(1 as libc::c_int);
        }
        i += 1
    }
    0 as libc::c_int
}
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_rw(mut b: *mut buf, mut write: libc::c_int) {
    let mut sector: uint64 =
        (*b).blockno
            .wrapping_mul((BSIZE / 512 as libc::c_int) as libc::c_uint) as uint64;
    acquire(&mut disk_global.0.vdisk_lock);
    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.
    // allocate the three descriptors.
    let mut idx: [libc::c_int; 3] = [0; 3];
    while !(alloc3_desc(idx.as_mut_ptr()) == 0 as libc::c_int) {
        sleep(
            &mut *disk_global
                .0
                .free
                .as_mut_ptr()
                .offset(0 as libc::c_int as isize) as *mut libc::c_char
                as *mut libc::c_void,
            &mut disk_global.0.vdisk_lock,
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
        buf0.type_0 = VIRTIO_BLK_T_OUT as uint32
    } else {
        buf0.type_0 = VIRTIO_BLK_T_IN as uint32
    } // write the disk
    buf0.reserved = 0 as libc::c_int as uint32;
    buf0.sector = sector;
    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    (*disk_global
        .0
        .desc
        .offset(idx[0 as libc::c_int as usize] as isize))
    .addr = kvmpa(&mut buf0 as *mut virtio_blk_outhdr as uint64); // device writes b->data
    (*disk_global
        .0
        .desc
        .offset(idx[0 as libc::c_int as usize] as isize))
    .len = ::core::mem::size_of::<virtio_blk_outhdr>() as libc::c_ulong as uint32; // device reads b->data
    (*disk_global
        .0
        .desc
        .offset(idx[0 as libc::c_int as usize] as isize))
    .flags = VRING_DESC_F_NEXT as uint16; // device writes the status
    (*disk_global
        .0
        .desc
        .offset(idx[0 as libc::c_int as usize] as isize))
    .next = idx[1 as libc::c_int as usize] as uint16;
    (*disk_global
        .0
        .desc
        .offset(idx[1 as libc::c_int as usize] as isize))
    .addr = (*b).data.as_mut_ptr() as uint64;
    (*disk_global
        .0
        .desc
        .offset(idx[1 as libc::c_int as usize] as isize))
    .len = BSIZE as uint32;
    if write != 0 {
        (*disk_global
            .0
            .desc
            .offset(idx[1 as libc::c_int as usize] as isize))
        .flags = 0 as libc::c_int as uint16
    } else {
        (*disk_global
            .0
            .desc
            .offset(idx[1 as libc::c_int as usize] as isize))
        .flags = VRING_DESC_F_WRITE as uint16
    }
    let fresh0 = &mut (*disk_global
        .0
        .desc
        .offset(idx[1 as libc::c_int as usize] as isize))
    .flags;
    *fresh0 = (*fresh0 as libc::c_int | VRING_DESC_F_NEXT) as uint16;
    (*disk_global
        .0
        .desc
        .offset(idx[1 as libc::c_int as usize] as isize))
    .next = idx[2 as libc::c_int as usize] as uint16;
    disk_global.0.info[idx[0 as libc::c_int as usize] as usize].status =
        0 as libc::c_int as libc::c_char;
    (*disk_global
        .0
        .desc
        .offset(idx[2 as libc::c_int as usize] as isize))
    .addr = &mut (*disk_global
        .0
        .info
        .as_mut_ptr()
        .offset(*idx.as_mut_ptr().offset(0 as libc::c_int as isize) as isize))
    .status as *mut libc::c_char as uint64;
    (*disk_global
        .0
        .desc
        .offset(idx[2 as libc::c_int as usize] as isize))
    .len = 1 as libc::c_int as uint32;
    (*disk_global
        .0
        .desc
        .offset(idx[2 as libc::c_int as usize] as isize))
    .flags = VRING_DESC_F_WRITE as uint16;
    (*disk_global
        .0
        .desc
        .offset(idx[2 as libc::c_int as usize] as isize))
    .next = 0 as libc::c_int as uint16;
    // record struct buf for virtio_disk_intr().
    (*b).disk = 1 as libc::c_int;
    disk_global.0.info[idx[0 as libc::c_int as usize] as usize].b = b;
    // avail[0] is flags
    // avail[1] tells the device how far to look in avail[2...].
    // avail[2...] are desc[] indices the device should process.
    // we only tell device the first index in our chain of descriptors.
    *disk_global.0.avail.offset(
        (2 as libc::c_int
            + *disk_global.0.avail.offset(1 as libc::c_int as isize) as libc::c_int % NUM)
            as isize,
    ) = idx[0 as libc::c_int as usize] as uint16; // value is queue number
    ::core::intrinsics::atomic_fence();
    *disk_global.0.avail.offset(1 as libc::c_int as isize) =
        (*disk_global.0.avail.offset(1 as libc::c_int as isize) as libc::c_int + 1 as libc::c_int)
            as uint16;
    ::core::ptr::write_volatile(
        (VIRTIO0 + 0x50 as libc::c_int) as *mut uint32,
        0 as libc::c_int as uint32,
    );
    // Wait for virtio_disk_intr() to say request has finished.
    while (*b).disk == 1 as libc::c_int {
        sleep(b as *mut libc::c_void, &mut disk_global.0.vdisk_lock); // disk is done with buf
    }
    disk_global.0.info[idx[0 as libc::c_int as usize] as usize].b = ptr::null_mut();
    free_chain(idx[0 as libc::c_int as usize]);
    release(&mut disk_global.0.vdisk_lock);
}
#[no_mangle]
pub unsafe extern "C" fn virtio_disk_intr() {
    acquire(&mut disk_global.0.vdisk_lock);
    while disk_global.0.used_idx as libc::c_int % NUM
        != (*disk_global.0.used).id as libc::c_int % NUM
    {
        let mut id: libc::c_int =
            (*disk_global.0.used).elems[disk_global.0.used_idx as usize].id as libc::c_int;
        if disk_global.0.info[id as usize].status as libc::c_int != 0 as libc::c_int {
            panic(
                b"virtio_disk_intr status\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        (*disk_global.0.info[id as usize].b).disk = 0 as libc::c_int;
        wakeup(disk_global.0.info[id as usize].b as *mut libc::c_void);
        disk_global.0.used_idx =
            ((disk_global.0.used_idx as libc::c_int + 1 as libc::c_int) % NUM) as uint16
    }
    release(&mut disk_global.0.vdisk_lock);
}
