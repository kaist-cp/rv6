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

use core::array::IntoIter;
use core::mem;
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use arrayvec::ArrayVec;

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
    desc: DescriptorPool,
    avail: *mut AvailableRing,
    used: *mut [UsedArea; NUM],

    used_idx: u16,

    /// track info about in-flight operations,
    /// for use when completion interrupt arrives.
    /// indexed by first descriptor index of chain.
    info: [InflightInfo; NUM],

    vdisk_lock: RawSpinlock,
}

struct DescriptorPool {
    desc: *mut [VRingDesc; NUM],

    /// our own book-keeping.
    free: [bool; NUM], // TODO : Disk can be implemented using bitmap
}

#[derive(Debug)]
struct Descriptor {
    idx: usize,
}

// It needs repr(C) because it's read by device
// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-380006
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
    status: bool,
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

impl Descriptor {
    unsafe fn new(idx: usize) -> Self {
        Self { idx }
    }
}

impl DescriptorPool {
    const fn zeroed() -> Self {
        Self {
            desc: ptr::null_mut(),
            free: [false; NUM],
        }
    }

    fn new(page: *mut Page) -> Self {
        Self {
            desc: page as _,
            free: [true; NUM],
        }
    }

    /// find a free descriptor, mark it non-free, return its index.
    fn alloc(&mut self) -> Option<Descriptor> {
        for (idx, free) in self.free.iter_mut().enumerate() {
            if *free {
                *free = false;
                return Some(unsafe { Descriptor::new(idx) });
            }
        }

        None
    }

    fn alloc3(&mut self) -> Option<[Descriptor; 3]> {
        let mut descs = ArrayVec::new();

        for _ in 0..3 {
            match self.alloc() {
                Some(desc) => descs.push(desc),
                None => {
                    for desc in descs {
                        self.free(desc);
                    }
                    return None;
                }
            }
        }

        Some(descs.into_inner().unwrap())
    }

    /// mark a descriptor as free.
    fn free(&mut self, desc: Descriptor) {
        let Descriptor { idx } = desc;
        assert!(idx < NUM, "virtio_disk_intr 1");
        assert!(!self.free[idx], "virtio_disk_intr 2");

        unsafe {
            (*self.desc)[idx].addr = 0;
            self.free[idx] = true;
            wakeup(&mut self.free as *mut _ as _);
        }
    }

    fn get(&self, desc: &Descriptor) -> &VRingDesc {
        unsafe { &(*self.desc)[desc.idx] }
    }

    fn get_mut(&self, desc: &mut Descriptor) -> &mut VRingDesc {
        unsafe { &mut (*self.desc)[desc.idx] }
    }
}

impl Disk {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            desc: DescriptorPool::zeroed(),
            avail: ptr::null_mut(),
            used: ptr::null_mut(),
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
            status: false,
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

    DISK.desc = DescriptorPool::new(&mut VIRTQUEUE[0]);
    DISK.avail = (VIRTQUEUE[0].as_mut_ptr() as *mut VRingDesc).add(NUM) as _;
    DISK.used = VIRTQUEUE[1].as_mut_ptr() as _;

    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}

pub unsafe fn virtio_disk_rw(b: *mut Buf, write: bool) {
    let sector: usize = (*b).blockno.wrapping_mul((BSIZE / 512) as u32) as _;

    DISK.vdisk_lock.acquire();

    // the spec says that legacy block operations use three
    // descriptors: one for type/reserved/sector, one for
    // the data, one for a 1-byte status result.

    // allocate the three descriptors.
    let mut desc = loop {
        match DISK.desc.alloc3() {
            Some(idx) => break idx,
            None => sleep(
                DISK.desc.free.as_mut_ptr() as *mut libc::CVoid,
                &mut DISK.vdisk_lock,
            ),
        }
    };

    // format the three descriptors.
    // qemu's virtio-blk.c reads them.
    let mut buf0 = VirtIOBlockOutHeader::new(write, sector);

    // buf0 is on a kernel stack, which is not direct mapped,
    // thus the call to kvmpa().
    *DISK.desc.get_mut(&mut desc[0]) = VRingDesc {
        addr: kvmpa(&mut buf0 as *mut _ as _),
        len: mem::size_of::<VirtIOBlockOutHeader>() as _,
        flags: VRingDescFlags::NEXT,
        next: desc[1].idx as _,
    };

    // device reads/writes b->data
    *DISK.desc.get_mut(&mut desc[1]) = VRingDesc {
        addr: (*b).data.as_mut_ptr() as _,
        len: BSIZE as _,
        flags: if write {
            VRingDescFlags::NEXT
        } else {
            VRingDescFlags::NEXT | VRingDescFlags::WRITE
        },
        next: desc[2].idx as _,
    };

    DISK.info[desc[0].idx].status = false;

    // device writes the status
    *DISK.desc.get_mut(&mut desc[2]) = VRingDesc {
        addr: &mut DISK.info[desc[0].idx].status as *mut _ as _,
        len: 1,
        flags: VRingDescFlags::WRITE,
        next: 0,
    };

    // record struct Buf for virtio_disk_intr().
    (*b).disk = 1;
    DISK.info[desc[0].idx].b = b;

    // we only tell device the first index in our chain of descriptors.
    (*DISK.avail).ring[(*DISK.avail).idx as usize % NUM] = desc[0].idx as _;
    fence(Ordering::SeqCst);
    (*DISK.avail).idx += 1;

    // value is queue number
    MmioRegs::QueueNotify.write(0);

    // Wait for virtio_disk_intr() to say request has finished.
    while (*b).disk == 1 {
        sleep(b as *mut libc::CVoid, &mut DISK.vdisk_lock);
    }
    DISK.info[desc[0].idx].b = ptr::null_mut();
    IntoIter::new(desc).for_each(|desc| DISK.desc.free(desc));
    DISK.vdisk_lock.release();
}

pub unsafe fn virtio_disk_intr() {
    DISK.vdisk_lock.acquire();
    while (DISK.used_idx as usize).wrapping_rem(NUM)
        != ((*DISK.used)[0].id as usize).wrapping_rem(NUM)
    {
        let id = (*DISK.used)[0].elems[DISK.used_idx as usize].id as usize;
        if DISK.info[id].status {
            panic!("virtio_disk_intr status");
        }
        (*DISK.info[id].b).disk = 0;

        // disk is done with Buf
        wakeup(DISK.info[id].b as *mut libc::CVoid);

        DISK.used_idx = (DISK.used_idx.wrapping_add(1)).wrapping_rem(NUM as _)
    }
    DISK.vdisk_lock.release();
}
