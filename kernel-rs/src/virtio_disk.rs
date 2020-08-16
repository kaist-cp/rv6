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
    proc::{sleep, wakeup},
    riscv::{PGSHIFT, PGSIZE},
    spinlock::RawSpinlock,
    virtio::*,
    vm::kvmpa,
};
use core::ptr;
use core::sync::atomic::{fence, Ordering};

/// the address of virtio mmio register r.
const fn r(r: usize) -> *mut u32 {
    VIRTIO0.wrapping_add(r) as *mut u32
}

// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
struct Disk {
    /// memory for virtio descriptors &c for queue 0.
    /// this is a global instead of allocated because it must
    /// be multiple contiguous pages, which kalloc()
    /// doesn't support, and page aligned.
    pages: [u8; 2usize.wrapping_mul(PGSIZE)],
    desc: *mut VRingDesc,
    avail: *mut u16,
    used: *mut UsedArea,

    /// our own book-keeping.
    free: [u8; NUM as usize],
    used_idx: u16,

    /// track info about in-flight operations,
    /// for use when completion interrupt arrives.
    /// indexed by first descriptor index of chain.
    info: [InflightInfo; NUM as usize],

    vdisk_lock: RawSpinlock,
}

#[derive(Copy, Clone)]
struct InflightInfo {
    b: *mut Buf,
    status: u8,
}

#[derive(Default, Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct virtio_blk_outhdr {
    typ: u32,
    reserved: u32,
    sector: usize,
}

impl Disk {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            pages: [0; 8192],
            desc: ptr::null_mut(),
            avail: ptr::null_mut(),
            used: ptr::null_mut(),
            free: [0; NUM as usize],
            used_idx: 0,
            info: [InflightInfo::zeroed(); NUM as usize],
            vdisk_lock: RawSpinlock::zeroed(),
        }
    }
}

impl InflightInfo {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            b: ptr::null_mut(),
            status: 0,
        }
    }
}

static mut DISK: Disk = Disk::zeroed();

pub unsafe fn virtio_disk_init() {
    let mut status: VirtIOStatus = VirtIOStatus::empty();
    DISK.vdisk_lock.initlock("virtio_disk");
    if *(r(VIRTIO_MMIO_MAGIC_VALUE)) != 0x74726976
        || *(r(VIRTIO_MMIO_VERSION)) != 1
        || *(r(VIRTIO_MMIO_DEVICE_ID)) != 2
        || *(r(VIRTIO_MMIO_VENDOR_ID)) != 0x554d4551
    {
        panic!("could not find virtio disk");
    }
    status.insert(VirtIOStatus::ACKNOWLEDGE);
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status.bits());
    status.insert(VirtIOStatus::DRIVER);
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status.bits());

    // negotiate features
    let mut features = VirtIOFeatures::from_bits_unchecked(*(r(VIRTIO_MMIO_DEVICE_FEATURES)));

    features.remove(VirtIOFeatures::BLK_F_RO);
    features.remove(VirtIOFeatures::BLK_F_SCSI);
    features.remove(VirtIOFeatures::BLK_F_CONFIG_WCE);
    features.remove(VirtIOFeatures::BLK_F_MQ);
    features.remove(VirtIOFeatures::F_ANY_LAYOUT);
    features.remove(VirtIOFeatures::RING_F_EVENT_IDX);
    features.remove(VirtIOFeatures::RING_F_INDIRECT_DESC);

    ::core::ptr::write_volatile(r(VIRTIO_MMIO_DRIVER_FEATURES), features.bits());

    // tell device that feature negotiation is complete.
    status.insert(VirtIOStatus::FEATURES_OK);
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status.bits());

    // tell device we're completely ready.
    status.insert(VirtIOStatus::DRIVER_OK);
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_STATUS), status.bits());
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_GUEST_PAGE_SIZE), PGSIZE as u32);

    // initialize queue 0.
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_SEL), 0);
    let max: u32 = *(r(VIRTIO_MMIO_QUEUE_NUM_MAX));
    if max == 0 {
        panic!("virtio disk has no queue 0");
    }
    if max < NUM as u32 {
        panic!("virtio disk max queue too short");
    }
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_NUM), NUM as u32);
    ptr::write_bytes(DISK.pages.as_mut_ptr(), 0, 1);
    ::core::ptr::write_volatile(
        r(VIRTIO_MMIO_QUEUE_PFN),
        (DISK.pages.as_mut_ptr() as usize >> PGSHIFT) as u32,
    );

    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * u16, then num * u16
    // used = pages + 4096 -- 2 * u16, then num * vRingUsedElem

    DISK.desc = DISK.pages.as_mut_ptr() as *mut VRingDesc;
    DISK.avail = (DISK.desc as *mut u8).add(NUM.wrapping_mul(::core::mem::size_of::<VRingDesc>()))
        as *mut u16;
    DISK.used = DISK.pages.as_mut_ptr().add(PGSIZE) as *mut UsedArea;
    for i in 0..NUM {
        DISK.free[i] = 1;
    }

    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}

/// find a free descriptor, mark it non-free, return its index.
unsafe fn alloc_desc() -> i32 {
    for i in 0..NUM {
        if DISK.free[i] != 0 {
            DISK.free[i] = 0;
            return i as i32;
        }
    }
    -1
}

/// mark a descriptor as free.
unsafe fn free_desc(i: i32) {
    if i >= NUM as i32 {
        panic!("virtio_disk_intr 1");
    }
    if DISK.free[i as usize] != 0 {
        panic!("virtio_disk_intr 2");
    }
    (*DISK.desc.offset(i as isize)).addr = 0;
    DISK.free[i as usize] = 1;
    wakeup(&mut *DISK.free.as_mut_ptr().offset(0) as *mut u8 as *mut libc::CVoid);
}

/// free a chain of descriptors.
unsafe fn free_chain(mut i: i32) {
    loop {
        free_desc(i);
        if (*DISK.desc.offset(i as isize)).flags & VRING_DESC_F_NEXT == 0 {
            break;
        }
        i = (*DISK.desc.offset(i as isize)).next as i32;
    }
}

unsafe fn alloc3_desc(idx: *mut i32) -> i32 {
    for i in 0..3 {
        *idx.offset(i as isize) = alloc_desc();
        if *idx.offset(i as isize) < 0 {
            for j in 0..i {
                free_desc(*idx.offset(j as isize));
            }
            return -1;
        }
    }
    0
}

pub unsafe fn virtio_disk_rw(mut b: *mut Buf, write: i32) {
    let sector: usize = (*b).blockno.wrapping_mul((BSIZE / 512) as u32) as usize;

    DISK.vdisk_lock.acquire();

    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.

    // allocate the three descriptors.
    let mut idx: [i32; 3] = [0; 3];

    while alloc3_desc(idx.as_mut_ptr()) != 0 {
        sleep(
            &mut *DISK.free.as_mut_ptr().offset(0) as *mut u8 as *mut libc::CVoid,
            &mut DISK.vdisk_lock,
        );
    }

    // format the three descriptors.
    // qemu's virtio-blk.c reads them.

    let mut buf0: virtio_blk_outhdr = Default::default();

    if write != 0 {
        // write the disk
        buf0.typ = VIRTIO_BLK_T_OUT
    } else {
        // read the disk
        buf0.typ = VIRTIO_BLK_T_IN
    }
    buf0.reserved = 0;
    buf0.sector = sector;

    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    (*DISK.desc.offset(idx[0] as isize)).addr = kvmpa(&mut buf0 as *mut virtio_blk_outhdr as usize);
    (*DISK.desc.offset(idx[0] as isize)).len = ::core::mem::size_of::<virtio_blk_outhdr>() as u32;
    (*DISK.desc.offset(idx[0] as isize)).flags = VRING_DESC_F_NEXT;
    (*DISK.desc.offset(idx[0] as isize)).next = idx[1] as u16;
    (*DISK.desc.offset(idx[1] as isize)).addr = (*b).data.as_mut_ptr() as usize;
    (*DISK.desc.offset(idx[1] as isize)).len = BSIZE as u32;
    if write != 0 {
        // device reads b->data
        (*DISK.desc.offset(idx[1] as isize)).flags = 0
    } else {
        // device writes b->data
        (*DISK.desc.offset(idx[1] as isize)).flags = VRING_DESC_F_WRITE as u16
    }

    let fresh0 = &mut (*DISK.desc.offset(idx[1] as isize)).flags;

    *fresh0 |= VRING_DESC_F_NEXT;

    (*DISK.desc.offset(idx[1] as isize)).next = idx[2] as u16;

    DISK.info[idx[0] as usize].status = 0;

    (*DISK.desc.offset(idx[2] as isize)).addr = &mut (*DISK
        .info
        .as_mut_ptr()
        .offset(*idx.as_mut_ptr().offset(0) as isize))
    .status as *mut u8 as usize;

    (*DISK.desc.offset(idx[2] as isize)).len = 1;

    // device writes the status
    (*DISK.desc.offset(idx[2] as isize)).flags = VRING_DESC_F_WRITE;

    (*DISK.desc.offset(idx[2] as isize)).next = 0;

    // record struct Buf for virtio_disk_intr().
    (*b).disk = 1;
    DISK.info[idx[0] as usize].b = b;

    // avail[0] is flags
    // avail[1] tells the device how far to look in avail[2...].
    // avail[2...] are desc[] indices the device should process.
    // we only tell device the first index in our chain of descriptors.
    *DISK
        .avail
        .add(((*DISK.avail.add(1) as usize).wrapping_rem(NUM)).wrapping_add(2)) = idx[0] as u16;
    fence(Ordering::SeqCst);
    *DISK.avail.add(1) = (*DISK.avail.add(1) as i32 + 1) as u16;

    // value is queue number
    ::core::ptr::write_volatile(r(VIRTIO_MMIO_QUEUE_NOTIFY), 0);

    // Wait for virtio_disk_intr() to say request has finished.
    while (*b).disk == 1 {
        sleep(b as *mut libc::CVoid, &mut DISK.vdisk_lock);
    }
    DISK.info[idx[0] as usize].b = ptr::null_mut();
    free_chain(idx[0]);
    DISK.vdisk_lock.release();
}
pub unsafe fn virtio_disk_intr() {
    DISK.vdisk_lock.acquire();
    while (DISK.used_idx as usize).wrapping_rem(NUM) != ((*DISK.used).id as usize).wrapping_rem(NUM)
    {
        let id: usize = (*DISK.used).elems[DISK.used_idx as usize].id as usize;
        if DISK.info[id].status as i32 != 0 {
            panic!("virtio_disk_intr status");
        }
        (*DISK.info[id].b).disk = 0;

        // disk is done with Buf
        wakeup(DISK.info[id].b as *mut libc::CVoid);

        DISK.used_idx = (DISK.used_idx.wrapping_add(1)).wrapping_rem(NUM as u16)
    }
    DISK.vdisk_lock.release();
}
