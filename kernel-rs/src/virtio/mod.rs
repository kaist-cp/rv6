//! virtio device definitions.
//! for both the mmio interface, and virtio descriptors.
//! only tested with qemu.
//! this is the "legacy" virtio interface.
//!
//! the virtio spec:
//! https:///docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.pdf

// virtio mmio control registers, mapped starting at 0x10001000.
// from qemu virtio_mmio.h

use core::ptr;

use bitflags::bitflags;

use crate::memlayout::VIRTIO0;

mod virtio_disk;

pub use virtio_disk::Disk;

#[repr(usize)]
enum MmioRegs {
    /// 0x74726976
    MagicValue = 0x000,
    /// version; 1 is legacy
    Version = 0x004,
    /// device type; 1 is net, 2 is disk
    DeviceId = 0x008,
    /// 0x554d4551
    VendorId = 0x00c,
    DeviceFeatures = 0x010,
    DriverFeatures = 0x020,
    /// page size for PFN, write-only
    GuestPageSize = 0x028,
    /// select queue, write-only
    QueueSel = 0x030,
    /// max size of current queue, read-only
    QueueNumMax = 0x034,
    /// size of current queue, write-only
    QueueNum = 0x038,
    /// physical page number for queue, read/write
    QueuePfn = 0x040,
    /// ready bit
    QueueReady = 0x044,
    /// write-only
    QueueNotify = 0x050,
    /// read-only
    InterruptStatus = 0x060,
    /// write-only
    InterruptAck = 0x064,
    /// read/write
    Status = 0x070,
}

impl MmioRegs {
    fn read(self) -> u32 {
        // It is safe because
        // * `src` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `src` is properly aligned, as self % 4 == 0.
        // * `src` points to a properly initialized value, as u32 does not have
        //   any internal structure to be initialized.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::read_volatile((VIRTIO0 as *mut u8).add(self as _) as _) }
    }

    unsafe fn write(self, src: u32) {
        // Usually, this is safe because
        // * `dst` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `dst` is properly aligned, as self % 4 == 0.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        // However, the caller should be aware of the side effects caused by the driver.
        unsafe { ptr::write_volatile((VIRTIO0 as *mut u8).add(self as _) as _, src) }
    }

    /// Checks the virtio disk's properties.
    pub fn check() {
        assert!(
            MmioRegs::MagicValue.read() == 0x74726976
                && MmioRegs::Version.read() == 1
                && MmioRegs::DeviceId.read() == 2
                && MmioRegs::VendorId.read() == 0x554d4551,
            "could not find virtio disk"
        );
    }

    /// Sets the virtio status.
    pub fn set_status(status: &VirtIOStatus) {
        unsafe {
            MmioRegs::Status.write(status.bits());
        }
    }

    /// Returns the device's virtio features.
    pub fn get_features() -> VirtIOFeatures {
        VirtIOFeatures::from_bits_truncate(MmioRegs::DeviceFeatures.read())
    }

    /// Sets the device's virtio features.
    pub fn set_features(features: &VirtIOFeatures) {
        unsafe {
            MmioRegs::DriverFeatures.write(features.bits());
        }
    }

    /// Sets the page size for PFN.
    pub fn set_pg_size(size: u32) {
        unsafe {
            MmioRegs::GuestPageSize.write(size);
        }
    }

    /// Selects the current queue.
    pub fn select_queue(num: u32) {
        unsafe {
            MmioRegs::QueueSel.write(num);
        }
    }

    /// Returns the max size of the current selected queue.
    pub fn get_max_queue() -> u32 {
        MmioRegs::QueueNumMax.read()
    }

    /// Sets the current selected queue's size.
    pub fn set_queue_size(size: u32) {
        unsafe {
            MmioRegs::QueueNum.write(size);
        }
    }

    /// Sets the physical page number of the current selected queue.
    pub fn set_queue_page_num(pg_num: u32) {
        unsafe {
            MmioRegs::QueuePfn.write(pg_num);
        }
    }

    /// Notifies the given queue number.
    ///
    /// # Safety
    ///
    /// After notifying the queue, the driver will read/write the address given through the descriptors.
    /// The caller must make sure not to give a wrong address.
    pub unsafe fn notify_queue(num: u32) {
        unsafe {
            MmioRegs::QueueNotify.write(num);
        }
    }

    /// Acknowledges all interrupts.
    pub fn intr_ack_all() {
        let intr_status = MmioRegs::InterruptStatus.read() & 0x3;
        unsafe {
            MmioRegs::InterruptAck.write(intr_status);
        }
    }
}

bitflags! {
    /// Status register bits, from qemu virtio_config.h
    struct VirtIOStatus: u32 {
        const ACKNOWLEDGE = 0b0001;
        const DRIVER = 0b0010;
        const DRIVER_OK = 0b0100;
        const FEATURES_OK = 0b1000;
    }
}

bitflags! {
    // Device feature bits
    struct VirtIOFeatures: u32 {
        /// Disk is read-only
        const BLK_F_RO = 1 << 5;

        /// Supports scsi command passthru
        const BLK_F_SCSI = 1 << 7;

        /// Writeback mode available in config
        const BLK_F_CONFIG_WCE = 1 << 11;

        /// support more than one vq
        const BLK_F_MQ = 1 << 12;

        const F_ANY_LAYOUT = 1 << 27;
        const RING_F_INDIRECT_DESC = 1 << 28;
        const RING_F_EVENT_IDX = 1 << 29;

        const ETC =
            !Self::BLK_F_RO.bits &
            !Self::BLK_F_SCSI.bits &
            !Self::BLK_F_CONFIG_WCE.bits &
            !Self::BLK_F_MQ.bits &
            !Self::F_ANY_LAYOUT.bits &
            !Self::RING_F_INDIRECT_DESC.bits &
            !Self::RING_F_EVENT_IDX.bits;
    }
}

/// This many virtio descriptors. It must be a power of two.
const NUM: usize = 1 << 3;

/// A single descriptor, from the spec.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-320005
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
#[derive(Copy, Clone)]
struct VirtqDesc {
    pub addr: usize,
    pub len: u32,
    pub flags: VirtqDescFlags,
    pub next: u16,
}

bitflags! {
    struct VirtqDescFlags: u16 {
        const FREED = 0b00;

        /// chained with another descriptor
        const NEXT = 0b01;

        /// device writes (vs read)
        const WRITE = 0b10;
    }
}

/// The (entire) avail ring, from the spec.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-380006
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct VirtqAvail {
    /// always zero
    pub flags: u16,

    /// Tells the device how far to look in `ring`.
    pub idx: u16,

    /// `desc` indices the device should process.
    pub ring: [u16; NUM],
}

/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-430008
// It must be page-aligned.
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
struct VirtqUsed {
    /// always zero
    pub flags: u16,

    /// device increments when it adds a ring[] entry
    pub id: u16,

    pub ring: [VirtqUsedElem; NUM],
}

/// One entry in the "used" ring, with which the device tells the driver about
/// completed requests.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-430008
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
#[derive(Copy, Clone)]
struct VirtqUsedElem {
    /// index of start of completed descriptor chain
    pub id: u32,

    pub len: u32,
}

/// for disk ops
/// read the disk
const VIRTIO_BLK_T_IN: u32 = 0;

/// write the disk
const VIRTIO_BLK_T_OUT: u32 = 1;

impl VirtqDesc {
    pub const fn zero() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: VirtqDescFlags::FREED,
            next: 0,
        }
    }
}

impl VirtqAvail {
    pub const fn zero() -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: [0; NUM],
        }
    }
}

impl VirtqUsed {
    pub const fn zero() -> Self {
        Self {
            flags: 0,
            id: 0,
            ring: [VirtqUsedElem::zero(); NUM],
        }
    }
}

impl VirtqUsedElem {
    pub const fn zero() -> Self {
        Self { id: 0, len: 0 }
    }
}
