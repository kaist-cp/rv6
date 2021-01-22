//! virtio device definitions.
//! for both the mmio interface, and virtio descriptors.
//! only tested with qemu.
//! this is the "legacy" virtio interface.
//!
//! the virtio spec:
//! https:///docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.pdf

// virtio mmio control registers, mapped starting at 0x10001000.
// from qemu virtio_mmio.h

use crate::memlayout::VIRTIO0;
use core::ptr;

#[repr(usize)]
pub enum MmioRegs {
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
    pub fn read(self) -> u32 {
        // It is safe because
        // * `src` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `src` is properly aligned, as self % 4 == 0.
        // * `src` points to a properly initialized value, as u32 does not have
        //   any internal structure to be initialized.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::read_volatile((VIRTIO0 as *mut u8).add(self as _) as _) }
    }

    pub fn write(self, src: u32) {
        // It is safe because
        // * `dst` is valid, as the kernel can access [VIRTIO0..VIRTIO0+PGSIZE).
        // * `dst` is properly aligned, as self % 4 == 0.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::write_volatile((VIRTIO0 as *mut u8).add(self as _) as _, src) }
    }
}

bitflags! {
    /// Status register bits, from qemu virtio_config.h
    pub struct VirtIOStatus: u32 {
        const ACKNOWLEDGE = 0b0001;
        const DRIVER = 0b0010;
        const DRIVER_OK = 0b0100;
        const FEATURES_OK = 0b1000;
    }
}

bitflags! {
    // Device feature bits
    pub struct VirtIOFeatures: u32 {
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
    }
}

/// This many virtio descriptors. It must be a power of two.
pub const NUM: usize = 1 << 3;

/// A single descriptor, from the spec.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-320005
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
#[derive(Copy, Clone)]
pub struct VirtqDesc {
    pub addr: usize,
    pub len: u32,
    pub flags: VirtqDescFlags,
    pub next: u16,
}

bitflags! {
    pub struct VirtqDescFlags: u16 {
        const FREED = 0b00;

        /// chained with another descriptor
        const NEXT = 0b01;

        /// device writes (vs read)
        const WRITE = 0b10;
    }
}

/// One entry in the "used" ring, with which the device tells the driver about
/// completed requests.
/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-430008
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
#[derive(Copy, Clone)]
pub struct VirtqUsedElem {
    /// index of start of completed descriptor chain
    pub id: u32,

    pub len: u32,
}

/// for disk ops
/// read the disk
pub const VIRTIO_BLK_T_IN: u32 = 0;

/// write the disk
pub const VIRTIO_BLK_T_OUT: u32 = 1;

/// https://docs.oasis-open.org/virtio/virtio/v1.1/csprd01/virtio-v1.1-csprd01.html#x1-430008
// It must be page-aligned.
// It needs repr(C) because it is read by device.
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C, align(4096))]
pub struct VirtqUsed {
    /// always zero
    pub flags: u16,

    /// device increments when it adds a ring[] entry
    pub id: u16,

    pub ring: [VirtqUsedElem; NUM],
}
