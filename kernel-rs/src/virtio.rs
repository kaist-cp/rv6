/// virtio device definitions.
/// for both the mmio interface, and virtio descriptors.
/// only tested with qemu.
/// this is the "legacy" virtio interface.
///
/// the virtio spec:
/// https:///docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.pdf

// virtio mmio control registers, mapped starting at 0x10001000.
// from qemu virtio_mmio.h

/// 0x74726976
pub const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;

/// version; 1 is legacy
pub const VIRTIO_MMIO_VERSION: usize = 0x004;

/// device type; 1 is net, 2 is disk
pub const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;

/// 0x554d4551
pub const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;

pub const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;

pub const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;

/// page size for PFN, write-only
pub const VIRTIO_MMIO_GUEST_PAGE_SIZE: usize = 0x028;

/// select queue, write-only
pub const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;

/// max size of current queue, read-only
pub const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;

/// size of current queue, write-only
pub const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;

/// physical page number for queue, read/write
pub const VIRTIO_MMIO_QUEUE_PFN: usize = 0x040;

/// ready bit
pub const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;

/// write-only
pub const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;

/// read/write
pub const VIRTIO_MMIO_STATUS: usize = 0x070;

/// status register bits, from qemu virtio_config.h
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u32 = 1;
pub const VIRTIO_CONFIG_S_DRIVER: u32 = 2;
pub const VIRTIO_CONFIG_S_DRIVER_OK: u32 = 4;
pub const VIRTIO_CONFIG_S_FEATURES_OK: u32 = 8;

/// device feature bits

/// Disk is read-only
pub const VIRTIO_BLK_F_RO: u32 = 5;

/// Supports scsi command passthru
pub const VIRTIO_BLK_F_SCSI: u32 = 7;

/// Writeback mode available in config
pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 11;

/// support more than one vq
pub const VIRTIO_BLK_F_MQ: u32 = 12;

pub const VIRTIO_F_ANY_LAYOUT: u32 = 27;

pub const VIRTIO_RING_F_INDIRECT_DESC: u32 = 28;

pub const VIRTIO_RING_F_EVENT_IDX: u32 = 29;

/// this many virtio descriptors.
/// must be a power of two.
pub const NUM: usize = 8;
#[derive(Copy, Clone)]
pub struct VRingDesc {
    pub addr: usize,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// chained with another descriptor
pub const VRING_DESC_F_NEXT: u16 = 1;

/// device writes (vs read)
pub const VRING_DESC_F_WRITE: u16 = 2;
#[derive(Copy, Clone)]
pub struct VRingUsedElem {
    /// index of start of completed descriptor chain
    pub id: u32,

    pub len: u32,
}

/// for disk ops
/// read the disk
pub const VIRTIO_BLK_T_IN: u32 = 0;

/// write the disk
pub const VIRTIO_BLK_T_OUT: u32 = 1;
#[derive(Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct UsedArea {
    pub flags: u16,
    pub id: u16,
    pub elems: [VRingUsedElem; NUM],
}
