/// driver for qemu's virtio disk device.
/// uses qemu's mmio interface to virtio.
/// qemu presents a "legacy" virtio interface.
///
/// qemu ... -drive file=fs.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
use crate::libc;
use crate::{
    buf::Buf,
    fs::BSIZE,
    memlayout::VIRTIO0,
    printf::panic,
    proc::{sleep, wakeup},
    riscv::{PGSHIFT, PGSIZE},
    spinlock::{acquire, initlock, release, Spinlock},
    virtio::*,
    vm::kvmpa,
};
use core::ptr;

/// the address of virtio mmio register r.
const fn r(r: i32) -> *mut u32 {
    (VIRTIO0 + r) as *mut u32
}

/// the address of virtio mmio register r.
#[derive(Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
pub struct Disk {
    /// memory for virtio descriptors &c for queue 0.
    /// this is a global instead of allocated because it must
    /// be multiple contiguous pages, which kalloc()
    /// doesn't support, and page aligned.
    pub pages: [libc::c_char; 2 * PGSIZE as usize],
    pub desc: *mut VRingDesc,
    pub avail: *mut u16,
    pub used: *mut UsedArea,

    /// our own book-keeping.
    pub free: [libc::c_char; NUM as usize],
    pub used_idx: u16,

    /// track info about in-flight operations,
    /// for use when completion interrupt arrives.
    /// indexed by first descriptor index of chain.
    pub info: [InflightInfo; NUM as usize],
    pub vdisk_lock: Spinlock,
}

#[derive(Copy, Clone)]
pub struct InflightInfo {
    pub b: *mut Buf,
    pub status: libc::c_char,
}

#[derive(Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct virtio_blk_outhdr {
    pub typ: u32,
    pub reserved: u32,
    pub sector: u64,
}

static mut disk: Disk = Disk {
    pages: [0; 8192],
    desc: 0 as *const VRingDesc as *mut VRingDesc,
    avail: 0 as *const u16 as *mut u16,
    used: 0 as *const UsedArea as *mut UsedArea,
    free: [0; NUM as usize],
    used_idx: 0,
    info: [InflightInfo {
        b: 0 as *const Buf as *mut Buf,
        status: 0,
    }; NUM as usize],
    vdisk_lock: Spinlock::zeroed(),
};

pub unsafe fn virtio_disk_init() {
    let mut status: u32 = 0 as u32;
    initlock(
        &mut disk.vdisk_lock,
        b"virtio_disk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    if *(r(VIRTIO_MMIO_MAGIC_VALUE)) != 0x74726976 as u32
        || *(r(VIRTIO_MMIO_VERSION)) != 1 as u32
        || *(r(VIRTIO_MMIO_DEVICE_ID)) != 2 as u32
        || *(r(VIRTIO_MMIO_VENDOR_ID)) != 0x554d4551 as u32
    {
        panic(
            b"could not find virtio disk\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    status |= VIRTIO_CONFIG_S_ACKNOWLEDGE as u32;
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status);
    status |= VIRTIO_CONFIG_S_DRIVER as u32;
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status);

    // negotiate features
    let mut features: u64 = *(r(VIRTIO_MMIO_DEVICE_FEATURES)) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_RO) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_SCSI) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_CONFIG_WCE) as u64;
    features &= !((1 as i32) << VIRTIO_BLK_F_MQ) as u64;
    features &= !((1 as i32) << VIRTIO_F_ANY_LAYOUT) as u64;
    features &= !((1 as i32) << VIRTIO_RING_F_EVENT_IDX) as u64;
    features &= !((1 as i32) << VIRTIO_RING_F_INDIRECT_DESC) as u64;
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_DRIVER_FEATURES), features as u32);

    // tell device that feature negotiation is complete.
    status |= VIRTIO_CONFIG_S_FEATURES_OK as u32;
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status);

    // tell device we're completely ready.
    status |= VIRTIO_CONFIG_S_DRIVER_OK as u32;
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status);
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_GUEST_PAGE_SIZE), PGSIZE as u32);

    // initialize queue 0.
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_SEL), 0);
    let mut max: u32 = *(r(VIRTIO_MMIO_QUEUE_NUM_MAX));
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
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_NUM), NUM as u32);
    ptr::write_bytes(disk.pages.as_mut_ptr(), 0, 1);
    ::core::ptr::write_volatile(
        r(VIRTIO_MMIO_QUEUE_PFN),
        (disk.pages.as_mut_ptr() as u64 >> PGSHIFT) as u32,
    );

    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * u16, then num * u16
    // used = pages + 4096 -- 2 * u16, then num * vRingUsedElem

    disk.desc = disk.pages.as_mut_ptr() as *mut VRingDesc;
    disk.avail = (disk.desc as *mut libc::c_char)
        .offset((NUM as u64).wrapping_mul(::core::mem::size_of::<VRingDesc>() as u64) as isize)
        as *mut u16;
    disk.used = disk.pages.as_mut_ptr().offset(PGSIZE as isize) as *mut UsedArea;
    let mut i: i32 = 0;
    while i < NUM {
        disk.free[i as usize] = 1 as i32 as libc::c_char;
        i += 1
    }

    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}

/// find a free descriptor, mark it non-free, return its index.
unsafe fn alloc_desc() -> i32 {
    let mut i: i32 = 0;
    while i < NUM {
        if disk.free[i as usize] != 0 {
            disk.free[i as usize] = 0 as i32 as libc::c_char;
            return i;
        }
        i += 1
    }
    -1
}

/// mark a descriptor as free.
unsafe fn free_desc(mut i: i32) {
    if i >= NUM {
        panic(b"virtio_disk_intr 1\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if disk.free[i as usize] != 0 {
        panic(b"virtio_disk_intr 2\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*disk.desc.offset(i as isize)).addr = 0 as i32 as u64;
    disk.free[i as usize] = 1 as i32 as libc::c_char;
    wakeup(
        &mut *disk.free.as_mut_ptr().offset(0 as i32 as isize) as *mut libc::c_char
            as *mut libc::c_void,
    );
}

/// free a chain of descriptors.
unsafe fn free_chain(mut i: i32) {
    loop {
        free_desc(i);
        if (*disk.desc.offset(i as isize)).flags as i32 & VRING_DESC_F_NEXT == 0 {
            break;
        }
        i = (*disk.desc.offset(i as isize)).next as i32
    }
}

unsafe fn alloc3_desc(mut idx: *mut i32) -> i32 {
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
pub unsafe fn virtio_disk_rw(mut b: *mut Buf, mut write: i32) {
    let mut sector: u64 = (*b).blockno.wrapping_mul((BSIZE / 512 as i32) as u32) as u64;

    acquire(&mut disk.vdisk_lock);

    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.

    // allocate the three descriptors.
    let mut idx: [i32; 3] = [0; 3];
    while alloc3_desc(idx.as_mut_ptr()) != 0 as i32 {
        sleep(
            &mut *disk.free.as_mut_ptr().offset(0 as i32 as isize) as *mut libc::c_char
                as *mut libc::c_void,
            &mut disk.vdisk_lock,
        );
    }

    // format the three descriptors.
    // qemu's virtio-blk.c reads them.
    let mut buf0: virtio_blk_outhdr = virtio_blk_outhdr {
        typ: 0,
        reserved: 0,
        sector: 0,
    };
    if write != 0 {
        // write the disk
        buf0.typ = VIRTIO_BLK_T_OUT as u32
    } else {
        // read the disk
        buf0.typ = VIRTIO_BLK_T_IN as u32
    }
    buf0.reserved = 0 as u32;
    buf0.sector = sector;

    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    (*disk.desc.offset(idx[0 as i32 as usize] as isize)).addr =
        kvmpa(&mut buf0 as *mut virtio_blk_outhdr as u64);
    (*disk.desc.offset(idx[0 as i32 as usize] as isize)).len =
        ::core::mem::size_of::<virtio_blk_outhdr>() as u64 as u32;
    (*disk.desc.offset(idx[0 as i32 as usize] as isize)).flags = VRING_DESC_F_NEXT as u16;
    (*disk.desc.offset(idx[0 as i32 as usize] as isize)).next = idx[1 as i32 as usize] as u16;
    (*disk.desc.offset(idx[1 as i32 as usize] as isize)).addr = (*b).data.as_mut_ptr() as u64;
    (*disk.desc.offset(idx[1 as i32 as usize] as isize)).len = BSIZE as u32;
    if write != 0 {
        // device writes b->data
        (*disk.desc.offset(idx[1 as i32 as usize] as isize)).flags = 0 as i32 as u16
    } else {
        // device reads b->data
        (*disk.desc.offset(idx[1 as i32 as usize] as isize)).flags = VRING_DESC_F_WRITE as u16
    }

    let fresh0 = &mut (*disk.desc.offset(idx[1 as i32 as usize] as isize)).flags;
    *fresh0 = (*fresh0 as i32 | VRING_DESC_F_NEXT) as u16;
    (*disk.desc.offset(idx[1 as i32 as usize] as isize)).next = idx[2 as i32 as usize] as u16;

    disk.info[idx[0 as i32 as usize] as usize].status = 0 as i32 as libc::c_char;
    (*disk.desc.offset(idx[2 as i32 as usize] as isize)).addr = &mut (*disk
        .info
        .as_mut_ptr()
        .offset(*idx.as_mut_ptr().offset(0 as i32 as isize) as isize))
    .status as *mut libc::c_char
        as u64;
    (*disk.desc.offset(idx[2 as i32 as usize] as isize)).len = 1 as u32;
    // device writes the status
    (*disk.desc.offset(idx[2 as i32 as usize] as isize)).flags = VRING_DESC_F_WRITE as u16;
    (*disk.desc.offset(idx[2 as i32 as usize] as isize)).next = 0 as i32 as u16;

    // record struct Buf for virtio_disk_intr().
    (*b).disk = 1;
    disk.info[idx[0 as i32 as usize] as usize].b = b;

    // avail[0] is flags
    // avail[1] tells the device how far to look in avail[2...].
    // avail[2...] are desc[] indices the device should process.
    // we only tell device the first index in our chain of descriptors.
    *disk
        .avail
        .offset((2 as i32 + *disk.avail.offset(1 as i32 as isize) as i32 % NUM) as isize) =
        idx[0 as i32 as usize] as u16;
    ::core::intrinsics::atomic_fence();
    *disk.avail.offset(1 as i32 as isize) =
        (*disk.avail.offset(1 as i32 as isize) as i32 + 1 as i32) as u16;

    // value is queue number
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_NOTIFY), 0);

    // Wait for virtio_disk_intr() to say request has finished.
    while (*b).disk == 1 {
        sleep(b as *mut libc::c_void, &mut disk.vdisk_lock);
    }
    disk.info[idx[0 as i32 as usize] as usize].b = ptr::null_mut();
    free_chain(idx[0 as i32 as usize]);
    release(&mut disk.vdisk_lock);
}
pub unsafe fn virtio_disk_intr() {
    acquire(&mut disk.vdisk_lock);
    while disk.used_idx as i32 % NUM != (*disk.used).id as i32 % NUM {
        let mut id: i32 = (*disk.used).elems[disk.used_idx as usize].id as i32;
        if disk.info[id as usize].status as i32 != 0 as i32 {
            panic(
                b"virtio_disk_intr status\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        (*disk.info[id as usize].b).disk = 0;

        // disk is done with Buf
        wakeup(disk.info[id as usize].b as *mut libc::c_void);

        disk.used_idx = ((disk.used_idx as i32 + 1 as i32) % NUM) as u16
    }
    release(&mut disk.vdisk_lock);
}
