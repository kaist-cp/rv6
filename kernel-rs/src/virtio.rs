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
pub const VIRTIO_MMIO_MAGIC_VALUE: i32 = 0x000;

/// version; 1 is legacy
pub const VIRTIO_MMIO_VERSION: i32 = 0x004;

/// device type; 1 is net, 2 is disk
pub const VIRTIO_MMIO_DEVICE_ID: i32 = 0x008;

/// 0x554d4551
pub const VIRTIO_MMIO_VENDOR_ID: i32 = 0x00c;
pub const VIRTIO_MMIO_DEVICE_FEATURES: i32 = 0x010;
pub const VIRTIO_MMIO_DRIVER_FEATURES: i32 = 0x020;

/// page size for PFN, write-only
pub const VIRTIO_MMIO_GUEST_PAGE_SIZE: i32 = 0x028;

/// select queue, write-only
pub const VIRTIO_MMIO_QUEUE_SEL: i32 = 0x030;

/// max size of current queue, read-only
pub const VIRTIO_MMIO_QUEUE_NUM_MAX: i32 = 0x034;

/// size of current queue, write-only
pub const VIRTIO_MMIO_QUEUE_NUM: i32 = 0x038;

/// physical page number for queue, read/write
pub const VIRTIO_MMIO_QUEUE_PFN: i32 = 0x040;

/// write-only
pub const VIRTIO_MMIO_QUEUE_NOTIFY: i32 = 0x050;

/// read/write
pub const VIRTIO_MMIO_STATUS: i32 = 0x070;

/// status register bits, from qemu virtio_config.h
pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: i32 = 1;
pub const VIRTIO_CONFIG_S_DRIVER: i32 = 2;
pub const VIRTIO_CONFIG_S_DRIVER_OK: i32 = 4;
pub const VIRTIO_CONFIG_S_FEATURES_OK: i32 = 8;

/// device feature bits
/// Disk is read-only
pub const VIRTIO_BLK_F_RO: i32 = 5;

/// Supports scsi command passthru
pub const VIRTIO_BLK_F_SCSI: i32 = 7;

/// Writeback mode available in config
pub const VIRTIO_BLK_F_CONFIG_WCE: i32 = 11;

/// support more than one vq
pub const VIRTIO_BLK_F_MQ: i32 = 12;
pub const VIRTIO_F_ANY_LAYOUT: i32 = 27;
pub const VIRTIO_RING_F_INDIRECT_DESC: i32 = 28;
pub const VIRTIO_RING_F_EVENT_IDX: i32 = 29;

/// this many virtio descriptors.
/// must be a power of two.
pub const NUM: i32 = 8;
#[derive(Copy, Clone)]
pub struct VRingDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// chained with another descriptor
pub const VRING_DESC_F_NEXT: i32 = 1;

/// device writes (vs read)
pub const VRING_DESC_F_WRITE: i32 = 2;
#[derive(Copy, Clone)]
pub struct VRingUsedElem {
    pub id: u32,

    /// index of start of completed descriptor chain
    pub len: u32,
}

/// for disk ops
/// read the disk
pub const VIRTIO_BLK_T_IN: i32 = 0;

/// write the disk
pub const VIRTIO_BLK_T_OUT: i32 = 1;
#[derive(Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct UsedArea {
    pub flags: u16,
    pub id: u16,
    pub elems: [VRingUsedElem; NUM as usize],
}
