///
/// virtio device definitions.
/// for both the mmio interface, and virtio descriptors.
/// only tested with qemu.
/// this is the "legacy" virtio interface.
///
/// the virtio spec:
/// https:///docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.pdf
///

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
#[repr(C)]
pub struct UsedArea {
    pub flags: u16,
    pub id: u16,
    pub elems: [VRingUsedElem; 8],
}
