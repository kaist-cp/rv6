/// Driver for qemu's virtio disk device.
/// Uses qemu's mmio interface to virtio.
/// qemu presents a "legacy" virtio interface.
///
/// qemu ... -drive file=fs.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
use crate::{
    bio::Buf,
    fs::BSIZE,
    kernel::kernel,
    memlayout::VIRTIO0,
    page::Page,
    riscv::{PGSHIFT, PGSIZE},
    sleepablelock::SleepablelockGuard,
    virtio::*,
    vm::{kvmpa, KVAddr, VAddr},
};

use core::array::IntoIter;
use core::mem;
use core::ops::{Deref, DerefMut};
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

pub struct Disk {
    desc: DescriptorPool,
    avail: *mut AvailableRing,
    used: *mut [UsedArea; NUM],

    used_idx: u16,

    /// Track info about in-flight operations,
    /// for use when completion interrupt arrives.
    /// indexed by first descriptor index of chain.
    info: [InflightInfo; NUM],
}

struct DescriptorPool {
    desc: *mut [VRingDesc; NUM],

    /// Our own book-keeping.
    free: [bool; NUM], // TODO : Disk can be implemented using bitmap
}

/// A descriptor allocated by driver.
///
/// Invariant: `ptr` must indicate `idx`-th descriptor of the original pool.
// TODO(@efenniht): `ptr` is redundant as the base pointer is stored in the pool. But if we remove
// it, the invariant of this type indirectly depends on the original pool (not appeared as a field).
#[derive(Debug)]
struct Descriptor {
    idx: usize,
    ptr: *mut VRingDesc,
}

// It needs repr(C) because it's read by device.
// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-380006
#[repr(C)]
struct AvailableRing {
    flags: u16,

    /// Tells the device how far to look in `ring`.
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
    unsafe fn new(idx: usize, ptr: *mut VRingDesc) -> Self {
        Self { idx, ptr }
    }
}

impl Deref for Descriptor {
    type Target = VRingDesc;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for Descriptor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Descriptor must never drop: use DescriptorPool::free instead.");
    }
}

impl DescriptorPool {
    const fn zero() -> Self {
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

    /// Find a free descriptor, mark it non-free, return its index.
    fn alloc(&mut self) -> Option<Descriptor> {
        for (idx, free) in self.free.iter_mut().enumerate() {
            if *free {
                *free = false;
                return Some(unsafe { Descriptor::new(idx, &mut (*self.desc)[idx]) });
            }
        }

        None
    }

    fn alloc_three_sectors(&mut self) -> Option<[Descriptor; 3]> {
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

        descs.into_inner().ok()
    }

    /// Mark a descriptor as free.
    fn free(&mut self, desc: Descriptor) {
        let Descriptor { idx, ptr } = desc;
        unsafe {
            assert!(
                (*self.desc).as_mut_ptr_range().contains(&ptr),
                "virtio_disk_intr 1",
            );
            assert!(!self.free[idx], "virtio_disk_intr 2");
            (*self.desc)[idx].addr = 0;
            self.free[idx] = true;
        }
        mem::forget(desc);
    }
}

impl Disk {
    pub const fn zero() -> Self {
        Self {
            desc: DescriptorPool::zero(),
            avail: ptr::null_mut(),
            used: ptr::null_mut(),
            used_idx: 0,
            info: [InflightInfo::zero(); NUM],
        }
    }

    // TODO: This should be removed after `WaitChannel::sleep` gets refactored to take `SpinlockGuard`.
    #[allow(clippy::while_immutable_condition)]
    pub unsafe fn virtio_rw(this: &mut SleepablelockGuard<'_, Self>, b: &mut Buf, write: bool) {
        let sector: usize = (*b).blockno.wrapping_mul((BSIZE / 512) as u32) as _;

        // The spec says that legacy block operations use three
        // descriptors: one for type/reserved/sector, one for
        // the data, one for a 1-byte status result.

        // Allocate the three descriptors.
        let mut desc = loop {
            match this.desc.alloc_three_sectors() {
                Some(idx) => break idx,
                None => {
                    this.wakeup();
                    this.sleep();
                }
            }
        };

        // Format the three descriptors.
        // qemu's virtio-blk.c reads them.
        let mut buf0 = VirtIOBlockOutHeader::new(write, sector);

        // buf0 is on a kernel stack, which is not direct mapped,
        // thus the call to kvmpa().
        *desc[0] = VRingDesc {
            addr: kvmpa(KVAddr::new(&mut buf0 as *mut _ as _)),
            len: mem::size_of::<VirtIOBlockOutHeader>() as _,
            flags: VRingDescFlags::NEXT,
            next: desc[1].idx as _,
        };

        // Device reads/writes b->data
        *desc[1] = VRingDesc {
            addr: b.deref_mut_inner().data.as_mut_ptr() as _,
            len: BSIZE as _,
            flags: if write {
                VRingDescFlags::NEXT
            } else {
                VRingDescFlags::NEXT | VRingDescFlags::WRITE
            },
            next: desc[2].idx as _,
        };

        this.info[desc[0].idx].status = false;

        // Device writes the status
        *desc[2] = VRingDesc {
            addr: &mut this.info[desc[0].idx].status as *mut _ as _,
            len: 1,
            flags: VRingDescFlags::WRITE,
            next: 0,
        };

        // Record struct Buf for virtio_disk_intr().
        b.deref_mut_inner().disk = true;
        this.info[desc[0].idx].b = b;

        // We only tell device the first index in our chain of descriptors.
        let ring_idx = (*this.avail).idx as usize % NUM;
        (*this.avail).ring[ring_idx] = desc[0].idx as _;
        fence(Ordering::SeqCst);
        (*this.avail).idx += 1;

        // Value is queue number.
        MmioRegs::QueueNotify.write(0);

        // Wait for virtio_disk_intr() to say request has finished.
        while b.deref_mut_inner().disk {
            (*b).vdisk_request_waitchannel.sleep_sleepable(this);
        }
        this.info[desc[0].idx].b = ptr::null_mut();
        IntoIter::new(desc).for_each(|desc| this.desc.free(desc));
        this.wakeup();
    }

    pub unsafe fn virtio_intr(&mut self) {
        while (self.used_idx as usize).wrapping_rem(NUM)
            != ((*self.used)[0].id as usize).wrapping_rem(NUM)
        {
            let id = (*self.used)[0].elems[self.used_idx as usize].id as usize;
            if self.info[id].status {
                panic!("virtio_self_intr status");
            }
            (*self.info[id].b).deref_mut_inner().disk = false;

            // Self is done with Buf.
            (*self.info[id].b).vdisk_request_waitchannel.wakeup();

            self.used_idx = (self.used_idx.wrapping_add(1)).wrapping_rem(NUM as _)
        }
    }
}

impl InflightInfo {
    const fn zero() -> Self {
        Self {
            b: ptr::null_mut(),
            status: false,
        }
    }
}

pub unsafe fn virtio_disk_init(virtqueue: &mut [Page; 2], disk: &mut Disk) {
    let mut status: VirtIOStatus = VirtIOStatus::empty();
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

    // Negotiate features
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

    // Tell device that feature negotiation is complete.
    status.insert(VirtIOStatus::FEATURES_OK);
    MmioRegs::Status.write(status.bits());

    // Tell device we're completely ready.
    status.insert(VirtIOStatus::DRIVER_OK);
    MmioRegs::Status.write(status.bits());
    MmioRegs::GuestPageSize.write(PGSIZE as _);

    // Initialize queue 0.
    MmioRegs::QueueSel.write(0);
    let max = MmioRegs::QueueNumMax.read();
    if max == 0 {
        panic!("virtio disk has no queue 0");
    }
    if max < NUM as u32 {
        panic!("virtio disk max queue too short");
    }
    MmioRegs::QueueNum.write(NUM as _);
    ptr::write_bytes(virtqueue, 0, 1);
    MmioRegs::QueuePfn.write((virtqueue.as_mut_ptr() as usize >> PGSHIFT) as _);

    // desc = pages -- num * VRingDesc
    // avail = pages + 0x40 -- 2 * u16, then num * u16
    // used = pages + 4096 -- 2 * u16, then num * vRingUsedElem

    disk.desc = DescriptorPool::new(&mut virtqueue[0]);
    disk.avail = (virtqueue[0].as_mut_ptr() as *mut VRingDesc).add(NUM) as _;
    disk.used = virtqueue[1].as_mut_ptr() as _;

    // plic.c and trap.c arrange for interrupts from VIRTIO0_IRQ.
}

pub unsafe fn virtio_disk_rw(b: &mut Buf, write: bool) {
    let mut disk = kernel().disk.lock();
    Disk::virtio_rw(&mut disk, b, write);
}

pub unsafe fn virtio_disk_intr() {
    kernel().disk.lock().virtio_intr();
}
