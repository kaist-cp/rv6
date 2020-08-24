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
    page::Page,
    proc::{sleep, wakeup},
    riscv::{PGSHIFT, PGSIZE},
    spinlock::RawSpinlock,
    virtio::*,
    vm::kvmpa,
};
use core::mem;
use core::ptr;
use core::sync::atomic::{fence, Ordering};

impl MmioRegs {
    unsafe fn read(self) -> u32 {
        ptr::read_volatile((VIRTIO0 as *mut u8).add(self as _) as _)
    }

    unsafe fn write(self, src: u32) {
        ptr::write_volatile((VIRTIO0 as *mut u8).add(self as _) as _, src)
    }
}

/// Memory for virtio descriptors `&c` for queue 0.
///
/// This is a global instead of allocated because it must be multiple contiguous pages, which
/// `kalloc()` doesn't support, and page aligned.
// TODO(efenniht): I moved out pages from Disk. Did I changed semantics (pointer indirection?)
static mut VIRTQUEUE: [Page; 2] = [Page::DEFAULT, Page::DEFAULT];

struct Disk {
    desc: *mut [VRingDesc; NUM],
    avail: *mut AvailableRing,
    used: *mut [UsedArea; NUM],

    /// our own book-keeping.
    free: [u8; NUM],
    used_idx: u16,

    /// track info about in-flight operations,
    /// for use when completion interrupt arrives.
    /// indexed by first descriptor index of chain.
    info: [InflightInfo; NUM],

    vdisk_lock: RawSpinlock,
}

// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct AvailableRing {
    flags: u16,

    /// tells the device how far to look in `ring`.
    idx: u16,

    /// `desc` indices the device should process.
    ring: [u16; NUM],
}

#[derive(Copy, Clone)]
struct InflightInfo {
    b: *mut Buf,
    status: u8,
}

// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct VirtIOBlockOutHeader {
    typ: u32,
    reserved: u32,
    sector: usize,
}

impl VirtIOBlockOutHeader {
    fn new(write: bool, sector: usize) -> Self {
        let typ = if write {
            VIRTIO_BLK_T_OUT
        } else {
            VIRTIO_BLK_T_IN
        };

        Self {
            typ,
            reserved: 0,
            sector,
        }
    }
}

impl Disk {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            desc: ptr::null_mut(),
            avail: ptr::null_mut(),
            used: ptr::null_mut(),
            free: [0; NUM],
            used_idx: 0,
            info: [InflightInfo::zeroed(); NUM],
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
    if !(MmioRegs::MagicValue.read() == 0x74726976
        && MmioRegs::Version.read() == 1
        && MmioRegs::DeviceId.read() == 2
        && MmioRegs::VendorId.read() == 0x554d4551)
    {
        panic!("could not find virtio disk");
    }
    status.insert(VirtIOStatus::ACKNOWLEDGE);
    MmioRegs::Status.write(status.bits());
    status.insert(VirtIOStatus::DRIVER);
    MmioRegs::Status.write(status.bits());

    // negotiate features
    let mut features = VirtIOFeatures::from_bits_unchecked(MmioRegs::DeviceFeatures.read());

    features.remove(
        VirtIOFeatures::BLK_F_RO
            | VirtIOFeatures::BLK_F_SCSI
            | VirtIOFeatures::BLK_F_CONFIG_WCE
            | VirtIOFeatures::BLK_F_MQ
            | VirtIOFeatures::F_ANY_LAYOUT
            | VirtIOFeatures::RING_F_EVENT_IDX
            | VirtIOFeatures::RING_F_INDIRECT_DESC,
    );

    MmioRegs::DriverFeatures.write(features.bits());

    // tell device that feature negotiation is complete.
    status.insert(VirtIOStatus::FEATURES_OK);
    MmioRegs::Status.write(status.bits());

    // tell device we're completely ready.
    status.insert(VirtIOStatus::DRIVER_OK);
    MmioRegs::Status.write(status.bits());
    MmioRegs::GuestPageSize.write(PGSIZE as _);

    // initialize queue 0.
    MmioRegs::QueueSel.write(0);
    let max = MmioRegs::QueueNumMax.read();
    if max == 0 {
        panic!("virtio disk has no queue 0");
    }
    if max < NUM as u32 {
        panic!("virtio disk max queue too short");
    }
    MmioRegs::QueueNum.write(NUM as _);
    ptr::write_bytes(&mut VIRTQUEUE, 0, 1);
    MmioRegs::QueuePfn.write((VIRTQUEUE.as_mut_ptr() as usize >> PGSHIFT) as _);

    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * u16, then num * u16
    // used = pages + 4096 -- 2 * u16, then num * vRingUsedElem

    DISK.desc = VIRTQUEUE[0].as_mut_ptr() as _;
    DISK.avail = (VIRTQUEUE[0].as_mut_ptr() as *mut VRingDesc).add(NUM) as _;
    DISK.used = VIRTQUEUE[1].as_mut_ptr() as _;
    for free in &mut DISK.free {
        *free = 1;
    }

    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}

/// find a free descriptor, mark it non-free, return its index.
unsafe fn alloc_desc() -> Option<i32> {
    for i in 0..NUM {
        if DISK.free[i] != 0 {
            DISK.free[i] = 0;
            return Some(i as _);
        }
    }
    None
}

/// mark a descriptor as free.
unsafe fn free_desc(i: i32) {
    if i >= NUM as i32 {
        panic!("virtio_disk_intr 1");
    }
    if DISK.free[i as usize] != 0 {
        panic!("virtio_disk_intr 2");
    }
    (*DISK.desc)[i as usize].addr = 0;
    DISK.free[i as usize] = 1;
    wakeup(&mut DISK.free as *mut _ as *mut libc::CVoid);
}

/// free a chain of descriptors.
unsafe fn free_chain(mut i: i32) {
    loop {
        free_desc(i);
        if !(*DISK.desc)[i as usize]
            .flags
            .contains(VRingDescFlags::NEXT)
        {
            break;
        }
        i = (*DISK.desc)[i as usize].next as i32;
    }
}

unsafe fn alloc3_desc() -> Option<[i32; 3]> {
    let mut idx = [0; 3];

    for i in 0..3 {
        match alloc_desc() {
            Some(desc) => idx[i] = desc,
            None => {
                for j in 0..i {
                    free_desc(idx[j]);
                }
                return None;
            }
        }
    }

    Some(idx)
}

pub unsafe fn virtio_disk_rw(b: *mut Buf, write: i32) {
    let sector: usize = (*b).blockno.wrapping_mul((BSIZE / 512) as u32) as usize;

    DISK.vdisk_lock.acquire();

    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.

    // allocate the three descriptors.
    let idx = loop {
        match alloc3_desc() {
            Some(idx) => break idx,
            None => sleep(
                DISK.free.as_mut_ptr() as *mut libc::CVoid,
                &mut DISK.vdisk_lock,
            ),
        }
    };

    // format the three descriptors.
    // qemu's virtio-blk.c reads them.
    let mut buf0 = VirtIOBlockOutHeader::new(write != 0, sector);

    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    (*DISK.desc)[idx[0] as usize] = VRingDesc {
        addr: kvmpa(&mut buf0 as *mut _ as usize),
        len: mem::size_of::<VirtIOBlockOutHeader>() as u32,
        flags: VRingDescFlags::NEXT,
        next: idx[1] as u16,
    };

    // device reads/writes b->data
    (*DISK.desc)[idx[1] as usize] = VRingDesc {
        addr: (*b).data.as_mut_ptr() as usize,
        len: BSIZE as u32,
        flags: if write != 0 {
            VRingDescFlags::NEXT
        } else {
            VRingDescFlags::NEXT | VRingDescFlags::WRITE
        },
        next: idx[2] as u16,
    };

    DISK.info[idx[0] as usize].status = 0;

    // device writes the status
    (*DISK.desc)[idx[2] as usize] = VRingDesc {
        addr: &mut DISK.info[idx[0] as usize].status as *mut _ as usize,
        len: 1,
        flags: VRingDescFlags::WRITE,
        next: 0,
    };

    // record struct Buf for virtio_disk_intr().
    (*b).disk = 1;
    DISK.info[idx[0] as usize].b = b;

    // we only tell device the first index in our chain of descriptors.
    (*DISK.avail).ring[(*DISK.avail).idx as usize % NUM] = idx[0] as u16;
    fence(Ordering::SeqCst);
    (*DISK.avail).idx += 1;

    // value is queue number
    MmioRegs::QueueNotify.write(0);

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
    while (DISK.used_idx as usize).wrapping_rem(NUM)
        != ((*DISK.used)[0].id as usize).wrapping_rem(NUM)
    {
        let id: usize = (*DISK.used)[0].elems[DISK.used_idx as usize].id as usize;
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
