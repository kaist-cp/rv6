// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

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

use crate::arch::memlayout::VIRTIO0;

mod virtio_disk;

pub use virtio_disk::VirtioDisk;

/// Memory mapped IO registers.
/// The kernel and virtio driver communicates to each other using these registers.
///
/// # Safety
///
/// * The `GuestPageSize` should be set to the page size of the guest architecture.
/// * All queues should be correctly initialized.
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
        // SAFETY:
        // * `src` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `src` is properly aligned, as self % 4 == 0.
        // * `src` points to a properly initialized value, as u32 does not have
        //   any internal structure to be initialized.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::read_volatile((VIRTIO0 as *mut u8).add(self as _) as _) }
    }

    /// # Safety
    ///
    /// Writing at memory mapped registers may cause hardware side effects.
    /// For example, after writing at `QueueNotify`, the virtio driver reads/writes the address given by the kernel.
    /// If a wrong address was given, this could lead to undefined behavior.
    unsafe fn write(self, dst: u32) {
        // SAFETY:
        // * `dst` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `dst` is properly aligned, as self % 4 == 0.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::write_volatile((VIRTIO0 as *mut u8).add(self as _) as _, dst) }
    }

    /// Checks the virtio disk's properties.
    fn check_virtio_disk() {
        assert!(
            MmioRegs::MagicValue.read() == 0x74726976,
            "could not find virtio disk"
        );
        assert!(MmioRegs::Version.read() == 1, "could not find virtio disk");
        assert!(MmioRegs::DeviceId.read() == 2, "could not find virtio disk");
        assert!(
            MmioRegs::VendorId.read() == 0x554d4551,
            "could not find virtio disk"
        );
    }

    /// Sets the virtio status.
    fn set_status(status: &VirtIOStatus) {
        // SAFETY: simply setting status bits does not cause side effects.
        unsafe {
            MmioRegs::Status.write(status.bits());
        }
    }

    /// Returns the device's virtio features.
    fn get_features() -> VirtIOFeatures {
        VirtIOFeatures::from_bits_truncate(MmioRegs::DeviceFeatures.read())
    }

    /// Sets the device's virtio features.
    fn set_features(features: &VirtIOFeatures) {
        // SAFETY: simply setting features bits does not cause side effects.
        unsafe {
            MmioRegs::DriverFeatures.write(features.bits());
        }
    }

    /// Sets the page size for PFN.
    ///
    /// # Safety
    ///
    /// The virtio driver will uses this info to calculate addresses.
    /// Hence, the caller must give the correct page size. Otherwise, the driver may read/write at wrong addresses.
    unsafe fn set_pg_size(size: u32) {
        // SAFETY: simply telling the page size does not cause side effects.
        unsafe {
            MmioRegs::GuestPageSize.write(size);
        }
    }

    /// Selects the queue `queue_num`, and initializes it with `queue_size` and `queue_addr`.
    ///
    /// # Safety
    ///
    /// The virtio driver will later use this info to read/write descriptors.
    /// Hence, the caller must give correct info.
    unsafe fn select_and_init_queue(queue_num: u32, queue_size: u32, queue_pg_num: u32) {
        // SAFETY: simply selecting and initializing the queue does not cause side effects.
        unsafe {
            MmioRegs::QueueSel.write(queue_num);
        }
        let max = MmioRegs::QueueNumMax.read();
        assert!(max != 0, "virtio disk has no queue {}", queue_num);
        assert!(max >= NUM as u32, "virtio disk max queue too short");

        unsafe {
            MmioRegs::QueueNum.write(queue_size);
            MmioRegs::QueuePfn.write(queue_pg_num);
        }
    }

    /// Notifies the given queue number.
    ///
    /// # Safety
    ///
    /// After notifying the queue, the driver will try to access the queue and read/write at the addresses given through descriptors.
    /// This may cause undefined behavior if the descriptors were not well set or contains wrong addresses.
    unsafe fn notify_queue(num: u32) {
        unsafe {
            MmioRegs::QueueNotify.write(num);
        }
    }

    /// Acknowledges all interrupts.
    fn intr_ack_all() {
        let intr_status = MmioRegs::InterruptStatus.read() & 0x3;
        // SAFETY: simply acknowledging interrupts does not cause undefined behavior.
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
    addr: usize,
    len: u32,
    flags: VirtqDescFlags,
    next: u16,
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
    flags: u16,

    /// Tells the device how far to look in `ring`.
    idx: u16,

    /// `desc` indices the device should process.
    ring: [u16; NUM],
}

/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-430008
// It must be page-aligned.
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
struct VirtqUsed {
    /// always zero
    flags: u16,

    /// device increments when it adds a ring[] entry
    id: u16,

    ring: [VirtqUsedElem; NUM],
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
    id: u32,

    len: u32,
}

/// for disk ops
/// read the disk
const VIRTIO_BLK_T_IN: u32 = 0;

/// write the disk
const VIRTIO_BLK_T_OUT: u32 = 1;

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
