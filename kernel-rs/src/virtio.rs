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
    pub unsafe fn read(self) -> u32 {
        ptr::read_volatile((VIRTIO0 as *mut u8).add(self as _) as _)
    }

    pub unsafe fn write(self, src: u32) {
        ptr::write_volatile((VIRTIO0 as *mut u8).add(self as _) as _, src)
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

/// this many virtio descriptors.
/// must be a power of two.
pub const NUM: usize = 8;

/// a single descriptor, from the spec.
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

/// One entry in the "used" ring, with which the
/// device tells the driver about completed requests.
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

// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub id: u16,
    pub ring: [VirtqUsedElem; NUM],
}

/// The format of the first descriptor in a disk request.
/// To be followed by two more descriptors containing
/// the block, and a one-byte status.
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct VirtIOBlockOutHeader {
    typ: u32,
    reserved: u32,
    sector: usize,
}

impl VirtIOBlockOutHeader {
    pub fn new(write: bool, sector: usize) -> Self {
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