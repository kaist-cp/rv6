/// Driver for qemu's virtio disk device.
/// Uses qemu's mmio interface to virtio.
/// qemu presents a "legacy" virtio interface.
///
/// qemu ... -drive file=fs.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
use crate::{
    bio::Buf,
    kernel::kernel,
    param::BSIZE,
    riscv::{PGSHIFT, PGSIZE},
    sleepablelock::{Sleepablelock, SleepablelockGuard},
    virtio::*,
};

use core::array::IntoIter;
use core::mem;
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use arrayvec::ArrayVec;

// It must be page-aligned.
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
pub struct Disk {
    /// The first region is a set (not a ring) of DMA descriptors, with which
    /// the driver tells the device where to read and write individual disk
    /// operations. There are NUM descriptors. Most commands consist of a
    /// "chain" (a linked list) of a couple of these descriptors.
    desc: [VirtqDesc; NUM],

    /// The next is a ring in which the driver writes descriptor numbers that
    /// the driver would like the device to process. It only includes the head
    /// descriptor of each chain. The ring has NUM elements.
    avail: VirtqAvail,

    /// Finally a ring in which the device writes descriptor numbers that the
    /// device has finished processing (just the head of each chain). There are
    /// NUM used ring entries.
    used: VirtqUsed,

    info: DiskInfo,
}

/// The (entire) avail ring, from the spec.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-380006
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct VirtqAvail {
    /// always zero
    flags: u16,

    /// Tells the device how far to look in `ring`.
    idx: u16,

    /// `desc` indices the device should process.
    ring: [u16; NUM],
}

// It must be page-aligned because a virtqueue (desc + avail + used) occupies
// two or more physically-contiguous pages.
#[repr(align(4096))]
struct DiskInfo {
    /// is a descriptor free?
    /// TODO(https://github.com/kaist-cp/rv6/issues/368): can be implemented with bitmap
    free: [bool; NUM],

    /// we've looked this far in used.
    used_idx: u16,

    /// Track info about in-flight operations, for use when completion
    /// interrupt arrives. Indexed by first descriptor index of chain.
    inflight: [InflightInfo; NUM],

    /// Disk command headers. One-for-one with descriptors, for convenience.
    ops: [VirtIOBlockOutHeader; NUM],
}

/// # Safety
///
/// `b` refers to a valid `Buf` unless it is null.
#[derive(Copy, Clone)]
struct InflightInfo {
    b: *mut Buf<'static>,
    status: bool,
}

/// The format of the first descriptor in a disk request. To be followed by two
/// more descriptors containing the block, and a one-byte status.
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
#[derive(Copy, Clone)]
struct VirtIOBlockOutHeader {
    typ: u32,
    reserved: u32,
    sector: usize,
}

impl Disk {
    pub const fn zero() -> Self {
        Self {
            desc: [VirtqDesc::zero(); NUM],
            avail: VirtqAvail::zero(),
            used: VirtqUsed::zero(),
            info: DiskInfo::zero(),
        }
    }
}

impl VirtqDesc {
    const fn zero() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: VirtqDescFlags::FREED,
            next: 0,
        }
    }
}

impl VirtqAvail {
    const fn zero() -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: [0; NUM],
        }
    }
}

impl VirtqUsed {
    const fn zero() -> Self {
        Self {
            flags: 0,
            id: 0,
            ring: [VirtqUsedElem::zero(); NUM],
        }
    }
}

impl VirtqUsedElem {
    const fn zero() -> Self {
        Self { id: 0, len: 0 }
    }
}

impl DiskInfo {
    const fn zero() -> Self {
        Self {
            free: [true; NUM],
            used_idx: 0,
            inflight: [InflightInfo::zero(); NUM],
            ops: [VirtIOBlockOutHeader::zero(); NUM],
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

impl VirtIOBlockOutHeader {
    const fn zero() -> Self {
        Self {
            typ: 0,
            reserved: 0,
            sector: 0,
        }
    }

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

/// A descriptor allocated by driver.
#[derive(Debug)]
struct Descriptor {
    idx: usize,
}

impl Descriptor {
    fn new(idx: usize) -> Self {
        Self { idx }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Descriptor must never drop. Use Disk::free instead.");
    }
}

impl Sleepablelock<Disk> {
    /// Return a locked Buf with the `latest` contents of the indicated block.
    /// If buf.valid is true, we don't need to access Disk.
    pub fn read(&self, dev: u32, blockno: u32) -> Buf<'static> {
        let mut buf = kernel().bcache.get_buf(dev, blockno).lock();
        if !buf.deref_inner().valid {
            Disk::rw(&mut self.lock(), &mut buf, false);
            buf.deref_inner_mut().valid = true;
        }
        buf
    }

    pub fn write(&self, b: &mut Buf<'static>) {
        Disk::rw(&mut self.lock(), b, true)
    }
}

impl Disk {
    pub fn init(&self) {
        let mut status: VirtIOStatus = VirtIOStatus::empty();

        // MMIO registers are located below KERNBASE, while kernel text and data
        // are located above KERNBASE, so we can safely read/write MMIO registers.
        assert!(
            MmioRegs::MagicValue.read() == 0x74726976
                && MmioRegs::Version.read() == 1
                && MmioRegs::DeviceId.read() == 2
                && MmioRegs::VendorId.read() == 0x554d4551,
            "could not find virtio disk"
        );
        status.insert(VirtIOStatus::ACKNOWLEDGE);
        MmioRegs::Status.write(status.bits());
        status.insert(VirtIOStatus::DRIVER);
        MmioRegs::Status.write(status.bits());

        // Negotiate features
        // It is safe because we just erase some bits and write the others back.
        // Since some feature bits are not defined in VirtIOFeatures, we use
        // `- (...)` instead of `& !(...)` to keep those bits. Note that `-` is
        // set difference (https://docs.rs/bitflags/1.2.1/bitflags/#operators).
        let features =
            unsafe { VirtIOFeatures::from_bits_unchecked(MmioRegs::DeviceFeatures.read()) }
                - (VirtIOFeatures::BLK_F_RO
                    | VirtIOFeatures::BLK_F_SCSI
                    | VirtIOFeatures::BLK_F_CONFIG_WCE
                    | VirtIOFeatures::BLK_F_MQ
                    | VirtIOFeatures::F_ANY_LAYOUT
                    | VirtIOFeatures::RING_F_EVENT_IDX
                    | VirtIOFeatures::RING_F_INDIRECT_DESC);

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
        assert!(max != 0, "virtio disk has no queue 0");
        assert!(max >= NUM as u32, "virtio disk max queue too short");
        MmioRegs::QueueNum.write(NUM as _);
        MmioRegs::QueuePfn.write((self.desc.as_ptr() as usize >> PGSHIFT) as _);

        // plic.rs and trap.rs arrange for interrupts from VIRTIO0_IRQ.
    }

    // This method reads and writes disk by reading and writing MMIO registers.
    // By the construction of the kernel page table in KernelMemory::new, the
    // virtual addresses of the MMIO registers are mapped to the proper physical
    // addresses. Therefore, this method is safe.
    fn rw(this: &mut SleepablelockGuard<'_, Self>, b: &mut Buf<'static>, write: bool) {
        let sector: usize = (*b).blockno as usize * (BSIZE / 512);

        // The spec's Section 5.2 says that legacy block operations use
        // three descriptors: one for type/reserved/sector, one for the
        // data, one for a 1-byte status result.

        // Allocate the three descriptors.
        let desc = loop {
            match this.alloc_three_descriptors() {
                Some(idx) => break idx,
                // We do not need wakeup for the None case:
                // * alloc_three_descriptors can be executed by one thread at
                //   once. Thus, we do not need to consider interleaving of
                //   alloc_three_descriptors.
                // * If alloc_three_descriptors fails, it frees only the
                //   descriptors that it created. It does not increase the
                //   number of free descriptors. Therefore, sleeping threads
                //   do not need to wake up, as alloc_three_descriptors will
                //   still fail.
                None => this.sleep(),
            }
        };

        // Format the three descriptors.
        // qemu's virtio-blk.c reads them.

        let buf0 = &mut this.info.ops[desc[0].idx];
        *buf0 = VirtIOBlockOutHeader::new(write, sector);

        this.desc[desc[0].idx] = VirtqDesc {
            addr: buf0 as *const _ as _,
            len: mem::size_of::<VirtIOBlockOutHeader>() as _,
            flags: VirtqDescFlags::NEXT,
            next: desc[1].idx as _,
        };

        // Device reads/writes b->data
        this.desc[desc[1].idx] = VirtqDesc {
            addr: b.deref_inner().data.as_ptr() as _,
            len: BSIZE as _,
            flags: if write {
                VirtqDescFlags::NEXT
            } else {
                VirtqDescFlags::NEXT | VirtqDescFlags::WRITE
            },
            next: desc[2].idx as _,
        };

        // device writes 0 on success
        this.info.inflight[desc[0].idx].status = true;

        // Device writes the status
        this.desc[desc[2].idx] = VirtqDesc {
            addr: &this.info.inflight[desc[0].idx].status as *const _ as _,
            len: 1,
            flags: VirtqDescFlags::WRITE,
            next: 0,
        };

        // Record struct Buf for virtio_disk_intr().
        b.deref_inner_mut().disk = true;
        // It does not break the invariant because b is &mut Buf, which refers
        // to a valid Buf.
        this.info.inflight[desc[0].idx].b = b;

        // Tell the device the first index in our chain of descriptors.
        let ring_idx = this.avail.idx as usize % NUM;
        this.avail.ring[ring_idx] = desc[0].idx as _;

        fence(Ordering::SeqCst);

        // Tell the device another avail ring entry is available.
        this.avail.idx += 1;

        fence(Ordering::SeqCst);

        // Value is queue number.
        MmioRegs::QueueNotify.write(0);

        // Wait for virtio_disk_intr() to say request has finished.
        while b.deref_inner().disk {
            (*b).vdisk_request_waitchannel.sleep(this);
        }
        // As it assigns null, the invariant of inflight is maintained even if
        // b: &mut Buf becomes invalid after this method returns.
        this.info.inflight[desc[0].idx].b = ptr::null_mut();
        IntoIter::new(desc).for_each(|desc| this.free(desc));
        this.wakeup();
    }

    pub fn intr(&mut self) {
        // The device won't raise another interrupt until we tell it
        // we've seen this interrupt, which the following line does.
        // This may race with the device writing new entries to
        // the "used" ring, in which case we may process the new
        // completion entries in this interrupt, and have nothing to do
        // in the next interrupt, which is harmless.
        MmioRegs::InterruptAck.write(MmioRegs::InterruptStatus.read() & 0x3);

        fence(Ordering::SeqCst);

        // The device increments disk.used->idx when it
        // adds an entry to the used ring.

        while self.info.used_idx != self.used.id {
            fence(Ordering::SeqCst);
            let id = self.used.ring[(self.info.used_idx as usize) % NUM].id as usize;

            assert!(!self.info.inflight[id].status, "Disk::intr status");

            // It is safe because, from the invariant, b refers to a valid
            // buffer unless it is null.
            let buf = unsafe { self.info.inflight[id].b.as_mut() }.expect("Disk::intr");

            // disk is done with buf
            buf.deref_inner_mut().disk = false;
            buf.vdisk_request_waitchannel.wakeup();

            self.info.used_idx += 1;
        }
    }

    /// Find a free descriptor, mark it non-free, return its index.
    fn alloc(&mut self) -> Option<Descriptor> {
        for (idx, free) in self.info.free.iter_mut().enumerate() {
            if *free {
                *free = false;
                return Some(Descriptor::new(idx));
            }
        }

        None
    }

    /// Allocate three descriptors (they need not be contiguous).
    /// Disk transfers always use three descriptors.
    fn alloc_three_descriptors(&mut self) -> Option<[Descriptor; 3]> {
        let mut descs = ArrayVec::<[_; 3]>::new();

        for _ in 0..3 {
            if let Some(desc) = self.alloc() {
                descs.push(desc);
            } else {
                for desc in descs {
                    self.free(desc);
                }
                return None;
            }
        }

        descs.into_inner().ok()
    }

    fn free(&mut self, desc: Descriptor) {
        let idx = desc.idx;
        assert!(!self.info.free[idx], "Disk::free");
        self.desc[idx].addr = 0;
        self.desc[idx].len = 0;
        self.desc[idx].flags = VirtqDescFlags::FREED;
        self.desc[idx].next = 0;
        self.info.free[idx] = true;
        mem::forget(desc);
    }
}
