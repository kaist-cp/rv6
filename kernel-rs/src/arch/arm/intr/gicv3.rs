// //! the ARM Generic Interrupt Controller v3 (GIC v3).

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use core::ptr;

use cortex_a::registers::*;
use tock_registers::interfaces::Writeable;

use crate::arch::{
    asm::{cpu_id, cpu_relax, isb, r_icc_ctlr_el1, r_mpidr},
    memlayout::{MemLayout, TIMER0_IRQ},
    timer::Timer,
};
use crate::memlayout::IrqNumbers;
use crate::param::NCPU;
use crate::timer::TimeManager;

// TODO: group all the constants properly as did in `gicv2.rs`,
// using `regiter_structs` macro.
pub const GICD_BASE: usize = 0x08000000;
pub const GICC_BASE: usize = 0x08010000;
pub const GICR_BASE: usize = 0x080a0000;

/*
 * Distributor registers. We assume we're running non-secure, with ARE
 * being set. Secure-only and non-ARE registers are not described.
 */
const GICD_CTLR: usize = 0x0000;
const GICD_TYPER: usize = 0x0004;
const GICD_IIDR: usize = 0x0008;
const GICD_STATUSR: usize = 0x0010;
const GICD_SETSPI_NSR: usize = 0x0040;
const GICD_CLRSPI_NSR: usize = 0x0048;
const GICD_SETSPI_SR: usize = 0x0050;
const GICD_CLRSPI_SR: usize = 0x0058;
const GICD_SEIR: usize = 0x0068;
const GICD_IGROUPR: usize = 0x0080;
const GICD_ISENABLER: usize = 0x0100;
const GICD_ICENABLER: usize = 0x0180;
const GICD_ISPENDR: usize = 0x0200;
const GICD_ICPENDR: usize = 0x0280;
const GICD_ISACTIVER: usize = 0x0300;
const GICD_ICACTIVER: usize = 0x0380;
const GICD_IPRIORITYR: usize = 0x0400;
const GICD_ICFGR: usize = 0x0C00;
const GICD_IGRPMODR: usize = 0x0D00;
const GICD_NSACR: usize = 0x0E00;
const GICD_IROUTER: usize = 0x6000;
const GICD_IDREGS: usize = 0xFFD0;
const GICD_PIDR2: usize = 0xFFE8;

/*
 * Those registers are actually from GICv2, but the spec demands that they
 * are implemented as RES0 if ARE is 1 (which we do in KVM's emulated GICv3).
 */
const GICD_ITARGETSR: usize = 0x0800;
const GICD_SGIR: usize = 0x0F00;
const GICD_CPENDSGIR: usize = 0x0F10;
const GICD_SPENDSGIR: usize = 0x0F20;

const GICD_CTLR_RWP: u32 = 1 << 31;
const GICD_CTLR_DS: u32 = 1 << 6;
const GICD_CTLR_ARE_NS: u32 = 1 << 4;
const GICD_CTLR_ENABLE_G1A: u32 = 1 << 1;
const GICD_CTLR_ENABLE_G1: u32 = 1 << 0;

const GICD_IIDR_IMPLEMENTER_SHIFT: u32 = 0;
const GICD_IIDR_IMPLEMENTER_MASK: u32 = 0xfff << GICD_IIDR_IMPLEMENTER_SHIFT;
const GICD_IIDR_REVISION_SHIFT: u32 = 12;
const GICD_IIDR_REVISION_MASK: u32 = 0xf << GICD_IIDR_REVISION_SHIFT;
const GICD_IIDR_VARIANT_SHIFT: u32 = 16;
const GICD_IIDR_VARIANT_MASK: u32 = 0xf << GICD_IIDR_VARIANT_SHIFT;
const GICD_IIDR_PRODUCT_ID_SHIFT: u32 = 24;
const GICD_IIDR_PRODUCT_ID_MASK: u32 = 0xff << GICD_IIDR_PRODUCT_ID_SHIFT;

/*
 * In systems with a single security state (what we emulate in KVM)
 * the meaning of the interrupt group enable bits is slightly different
 */
const GICD_CTLR_ENABLE_SS_G1: u32 = 1 << 1;
const GICD_CTLR_ENABLE_SS_G0: u32 = 1 << 0;

const GICD_TYPER_RSS: u32 = 1 << 26;
const GICD_TYPER_LPIS: u32 = 1 << 17;
const GICD_TYPER_MBIS: u32 = 1 << 16;

// const GICD_TYPER_ID_BITS:usize =(typer)	((((typer) >> 19) & 0x1f) + 1);
// const GICD_TYPER_NUM_LPIS:usize =(typer)	((((typer) >> 11) & 0x1f) + 1);
// const GICD_TYPER_IRQS:usize =(typer)		((((typer) & 0x1f) + 1) * 32);

const GICD_IROUTER_SPI_MODE_ONE: u32 = 0 << 31;
const GICD_IROUTER_SPI_MODE_ANY: u32 = 1 << 31;

const GIC_PIDR2_ARCH_MASK: u32 = 0xf0;
const GIC_PIDR2_ARCH_GICV3: u32 = 0x30;
const GIC_PIDR2_ARCH_GICV4: u32 = 0x40;

const GIC_V3_DIST_SIZE: u32 = 0x10000;

/*
 * Re-Distributor registers, offsets from RD_base
 */
const GICR_CTLR: usize = GICD_CTLR;
const GICR_IIDR: usize = 0x0004;
const GICR_TYPER: usize = 0x0008;
const GICR_STATUSR: usize = GICD_STATUSR;
const GICR_WAKER: usize = 0x0014;
const GICR_SETLPIR: usize = 0x0040;
const GICR_CLRLPIR: usize = 0x0048;
const GICR_SEIR: usize = GICD_SEIR;
const GICR_PROPBASER: usize = 0x0070;
const GICR_PENDBASER: usize = 0x0078;
const GICR_INVLPIR: usize = 0x00A0;
const GICR_INVALLR: usize = 0x00B0;
const GICR_SYNCR: usize = 0x00C0;
const GICR_MOVLPIR: usize = 0x0100;
const GICR_MOVALLR: usize = 0x0110;
const GICR_IDREGS: usize = GICD_IDREGS;
const GICR_PIDR2: usize = GICD_PIDR2;

const GICR_CTLR_ENABLE_LPIS: u32 = 1 << 0;
const GICR_CTLR_RWP: u64 = 1 << 3;

// const GICR_TYPER_CPU_NUMBER:usize =(r)	(((r) >> 8) & 0xffff);

const GICR_WAKER_PROC_SLEEP: usize = 1 << 1;
const GICR_WAKER_CHILDREN_ASLEEP: usize = 1 << 2;

const GIC_BASER_CACHE_NCNB: u64 = 0;
const GIC_BASER_CACHE_SAME_AS_INNER: u64 = 0;
const GIC_BASER_CACHE_NC: usize = 1;
const GIC_BASER_CACHE_RAWT: u64 = 2;
const GIC_BASER_CACHE_RAWB: u64 = 3;
const GIC_BASER_CACHE_WAWT: u64 = 4;
const GIC_BASER_CACHE_WAWB: u64 = 5;
const GIC_BASER_CACHE_RAWAWT: u64 = 6;
const GIC_BASER_CACHE_RAWAWB: u64 = 7;
const GIC_BASER_CACHE_MASK: u64 = 7;
const GIC_BASER_NON_SHAREABLE: u64 = 0;
const GIC_BASER_INNER_SHAREABLE: u64 = 1;
const GIC_BASER_OUTER_SHAREABLE: u64 = 2;
const GIC_BASER_SHAREABILITY_MASK: u64 = 3;

const GICR_PROPBASER_SHAREABILITY_SHIFT: u32 = 10;
const GICR_PROPBASER_INNER_CACHEABILITY_SHIFT: u32 = 7;
const GICR_PROPBASER_OUTER_CACHEABILITY_SHIFT: u32 = 56;
// const GICR_PROPBASER_SHAREABILITY_MASK:usize =				\;
// 	GIC_BASER_SHAREABILITY(GICR_PROPBASER, SHAREABILITY_MASK)
// const GICR_PROPBASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, MASK)
// const GICR_PROPBASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_PROPBASER, OUTER, MASK)
// const GICR_PROPBASER_CACHEABILITY_MASK:usize = GICR_PROPBASER_INNER_CACHEABILITY_MASK;

// const GICR_PROPBASER_InnerShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GICR_PROPBASER, InnerShareable)

// const GICR_PROPBASER_nCnB:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, nCnB);
// const GICR_PROPBASER_nC:usize = 	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, nC);
// const GICR_PROPBASER_RaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, RaWt);
// const GICR_PROPBASER_RaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, RaWb);
// const GICR_PROPBASER_WaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, WaWt);
// const GICR_PROPBASER_WaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, WaWb);
// const GICR_PROPBASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, RaWaWt);
// const GICR_PROPBASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PROPBASER, INNER, RaWaWb);

// const GICR_PROPBASER_IDBITS_MASK:usize =			(0x1f);
// const GICR_PROPBASER_ADDRESS:usize =(x)	((x) & GENMASK_ULL(51, 12));
// const GICR_PENDBASER_ADDRESS:usize =(x)	((x) & GENMASK_ULL(51, 16));

// const GICR_PENDBASER_SHAREABILITY_SHIFT:usize =		(10);
// const GICR_PENDBASER_INNER_CACHEABILITY_SHIFT:usize =		(7);
// const GICR_PENDBASER_OUTER_CACHEABILITY_SHIFT:usize =		(56);
// const GICR_PENDBASER_SHAREABILITY_MASK:usize =				\;
// 	GIC_BASER_SHAREABILITY(GICR_PENDBASER, SHAREABILITY_MASK)
// const GICR_PENDBASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, MASK)
// const GICR_PENDBASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_PENDBASER, OUTER, MASK)
// const GICR_PENDBASER_CACHEABILITY_MASK:usize = GICR_PENDBASER_INNER_CACHEABILITY_MASK;

// const GICR_PENDBASER_InnerShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GICR_PENDBASER, InnerShareable)

// const GICR_PENDBASER_nCnB:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, nCnB);
// const GICR_PENDBASER_nC:usize = 	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, nC);
// const GICR_PENDBASER_RaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, RaWt);
// const GICR_PENDBASER_RaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, RaWb);
// const GICR_PENDBASER_WaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, WaWt);
// const GICR_PENDBASER_WaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, WaWb);
// const GICR_PENDBASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, RaWaWt);
// const GICR_PENDBASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GICR_PENDBASER, INNER, RaWaWb);

// const GICR_PENDBASER_PTZ:usize =				BIT_ULL(62);

/*
 * Re-Distributor registers, offsets from SGI_base
 */
const GICR_IGROUPR0: usize = GICD_IGROUPR;
const GICR_ISENABLER0: usize = GICD_ISENABLER;
const GICR_ICENABLER0: usize = GICD_ICENABLER;
const GICR_ISPENDR0: usize = GICD_ISPENDR;
const GICR_ICPENDR0: usize = GICD_ICPENDR;
const GICR_ISACTIVER0: usize = GICD_ISACTIVER;
const GICR_ICACTIVER0: usize = GICD_ICACTIVER;
const GICR_IPRIORITYR0: usize = GICD_IPRIORITYR;
const GICR_ICFGR0: usize = GICD_ICFGR;
const GICR_IGRPMODR0: usize = GICD_IGRPMODR;
const GICR_NSACR: usize = GICD_NSACR;

const GICR_TYPER_PLPIS: u32 = 1 << 0;
const GICR_TYPER_VLPIS: u32 = 1 << 1;
const GICR_TYPER_DIRECT_LPIS: u32 = 1 << 3;
const GICR_TYPER_LAST: u32 = 1 << 4;

const GIC_V3_REDIST_SIZE: u32 = 0x20000;

const LPI_PROP_GROUP1: u32 = 1 << 1;
const LPI_PROP_ENABLED: u32 = 1 << 0;

/*
 * Re-Distributor registers, offsets from VLPI_base
 */
const GICR_VPROPBASER: u32 = 0x0070;

const GICR_VPROPBASER_IDBITS_MASK: u32 = 0x1f;

const GICR_VPROPBASER_SHAREABILITY_SHIFT: u32 = 10;
const GICR_VPROPBASER_INNER_CACHEABILITY_SHIFT: u32 = 7;
const GICR_VPROPBASER_OUTER_CACHEABILITY_SHIFT: u32 = 56;

// const GICR_VPROPBASER_SHAREABILITY_MASK:usize =				\;
// 	GIC_BASER_SHAREABILITY(GICR_VPROPBASER, SHAREABILITY_MASK)
// const GICR_VPROPBASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, MASK)
// const GICR_VPROPBASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, OUTER, MASK)
// const GICR_VPROPBASER_CACHEABILITY_MASK:usize =				\;
// 	GICR_VPROPBASER_INNER_CACHEABILITY_MASK

// const GICR_VPROPBASER_InnerShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GICR_VPROPBASER, InnerShareable)

// const GICR_VPROPBASER_nCnB:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, nCnB);
// const GICR_VPROPBASER_nC:usize = 	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, nC);
// const GICR_VPROPBASER_RaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, RaWt);
// const GICR_VPROPBASER_RaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, RaWb);
// const GICR_VPROPBASER_WaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, WaWt);
// const GICR_VPROPBASER_WaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, WaWb);
// const GICR_VPROPBASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, RaWaWt);
// const GICR_VPROPBASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPROPBASER, INNER, RaWaWb);

const GICR_VPENDBASER: u32 = 0x0078;

// const GICR_VPENDBASER_SHAREABILITY_SHIFT:usize =		(10);
// const GICR_VPENDBASER_INNER_CACHEABILITY_SHIFT:usize =	(7);
// const GICR_VPENDBASER_OUTER_CACHEABILITY_SHIFT:usize =	(56);
// const GICR_VPENDBASER_SHAREABILITY_MASK:usize =				\;
// 	GIC_BASER_SHAREABILITY(GICR_VPENDBASER, SHAREABILITY_MASK)
// const GICR_VPENDBASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, MASK)
// const GICR_VPENDBASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, OUTER, MASK)
// const GICR_VPENDBASER_CACHEABILITY_MASK:usize =				\;
// 	GICR_VPENDBASER_INNER_CACHEABILITY_MASK

// const GICR_VPENDBASER_NonShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GICR_VPENDBASER, NonShareable)

// const GICR_VPENDBASER_nCnB:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, nCnB);
// const GICR_VPENDBASER_nC:usize = 	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, nC);
// const GICR_VPENDBASER_RaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, RaWt);
// const GICR_VPENDBASER_RaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, RaWb);
// const GICR_VPENDBASER_WaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, WaWt);
// const GICR_VPENDBASER_WaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, WaWb);
// const GICR_VPENDBASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, RaWaWt);
// const GICR_VPENDBASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GICR_VPENDBASER, INNER, RaWaWb);

// const GICR_VPENDBASER_Dirty:usize =		(1ULL << 60);
// const GICR_VPENDBASER_PendingLast:usize =	(1ULL << 61);
// const GICR_VPENDBASER_IDAI:usize =		(1ULL << 62);
// const GICR_VPENDBASER_Valid:usize =		(1ULL << 63);

/*
 * ITS registers, offsets from ITS_base
 */
// const GITS_CTLR:usize =			0x0000;
// const GITS_IIDR:usize =			0x0004;
// const GITS_TYPER:usize =			0x0008;
// const GITS_CBASER:usize =			0x0080;
// const GITS_CWRITER:usize =			0x0088;
// const GITS_CREADR:usize =			0x0090;
// const GITS_BASER:usize =			0x0100;
// const GITS_IDREGS_BASE:usize =		0xffd0;
// const GITS_PIDR0:usize =			0xffe0;
// const GITS_PIDR1:usize =			0xffe4;
// const GITS_PIDR2:usize =			GICR_PIDR2;
// const GITS_PIDR4:usize =			0xffd0;
// const GITS_CIDR0:usize =			0xfff0;
// const GITS_CIDR1:usize =			0xfff4;
// const GITS_CIDR2:usize =			0xfff8;
// const GITS_CIDR3:usize =			0xfffc;

// const GITS_TRANSLATER:usize =			0x10040;

// const GITS_CTLR_ENABLE:usize =		(1U << 0);
// const GITS_CTLR_ImDe:usize =			(1U << 1);
// const	GITS_CTLR_ITS_NUMBER_SHIFT:usize =	4;
// const	GITS_CTLR_ITS_NUMBER:usize =		(0xFU << GITS_CTLR_ITS_NUMBER_SHIFT);
// const GITS_CTLR_QUIESCENT:usize =		(1U << 31);

// const GITS_TYPER_PLPIS:usize =		(1UL << 0);
// const GITS_TYPER_VLPIS:usize =		(1UL << 1);
// const GITS_TYPER_ITT_ENTRY_SIZE_SHIFT:usize =	4;
// const GITS_TYPER_ITT_ENTRY_SIZE:usize =(r)	((((r) >> GITS_TYPER_ITT_ENTRY_SIZE_SHIFT) & 0xf) + 1);
// const GITS_TYPER_IDBITS_SHIFT:usize =		8;
// const GITS_TYPER_DEVBITS_SHIFT:usize =	13;
// const GITS_TYPER_DEVBITS:usize =(r)		((((r) >> GITS_TYPER_DEVBITS_SHIFT) & 0x1f) + 1);
// const GITS_TYPER_PTA:usize =			(1UL << 19);
// const GITS_TYPER_HCC_SHIFT:usize =		24;
// const GITS_TYPER_HCC:usize =(r)		(((r) >> GITS_TYPER_HCC_SHIFT) & 0xff);
// const GITS_TYPER_VMOVP:usize =		(1ULL << 37);

// const GITS_IIDR_REV_SHIFT:usize =		12;
// const GITS_IIDR_REV_MASK:usize =		(0xf << GITS_IIDR_REV_SHIFT);
// const GITS_IIDR_REV:usize =(r)		(((r) >> GITS_IIDR_REV_SHIFT) & 0xf);
// const GITS_IIDR_PRODUCTID_SHIFT:usize =	24;

// const GITS_CBASER_VALID:usize =			(1ULL << 63);
// const GITS_CBASER_SHAREABILITY_SHIFT:usize =		(10);
// const GITS_CBASER_INNER_CACHEABILITY_SHIFT:usize =	(59);
// const GITS_CBASER_OUTER_CACHEABILITY_SHIFT:usize =	(53);
// const GITS_CBASER_SHAREABILITY_MASK:usize =					\;
// 	GIC_BASER_SHAREABILITY(GITS_CBASER, SHAREABILITY_MASK)
// const GITS_CBASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, MASK)
// const GITS_CBASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GITS_CBASER, OUTER, MASK)
// const GITS_CBASER_CACHEABILITY_MASK:usize = GITS_CBASER_INNER_CACHEABILITY_MASK;

// const GITS_CBASER_InnerShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GITS_CBASER, InnerShareable)

// const GITS_CBASER_nCnB:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, nCnB);
// const GITS_CBASER_nC:usize =		GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, nC);
// const GITS_CBASER_RaWt:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, RaWt);
// const GITS_CBASER_RaWb:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, RaWb);
// const GITS_CBASER_WaWt:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, WaWt);
// const GITS_CBASER_WaWb:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, WaWb);
// const GITS_CBASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, RaWaWt);
// const GITS_CBASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GITS_CBASER, INNER, RaWaWb);

// const GITS_BASER_NR_REGS:usize =		8;

// const GITS_BASER_VALID:usize =			(1ULL << 63);
// const GITS_BASER_INDIRECT:usize =			(1ULL << 62);

// const GITS_BASER_INNER_CACHEABILITY_SHIFT:usize =	(59);
// const GITS_BASER_OUTER_CACHEABILITY_SHIFT:usize =	(53);
// const GITS_BASER_INNER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GITS_BASER, INNER, MASK)
// const GITS_BASER_CACHEABILITY_MASK:usize =		GITS_BASER_INNER_CACHEABILITY_MASK;
// const GITS_BASER_OUTER_CACHEABILITY_MASK:usize =				\;
// 	GIC_BASER_CACHEABILITY(GITS_BASER, OUTER, MASK)
// const GITS_BASER_SHAREABILITY_MASK:usize =					\;
// 	GIC_BASER_SHAREABILITY(GITS_BASER, SHAREABILITY_MASK)

// const GITS_BASER_nCnB:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, nCnB);
// const GITS_BASER_nC:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, nC);
// const GITS_BASER_RaWt:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, RaWt);
// const GITS_BASER_RaWb:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, RaWb);
// const GITS_BASER_WaWt:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, WaWt);
// const GITS_BASER_WaWb:usize =		GIC_BASER_CACHEABILITY(GITS_BASER, INNER, WaWb);
// const GITS_BASER_RaWaWt:usize =	GIC_BASER_CACHEABILITY(GITS_BASER, INNER, RaWaWt);
// const GITS_BASER_RaWaWb:usize =	GIC_BASER_CACHEABILITY(GITS_BASER, INNER, RaWaWb);

// const GITS_BASER_TYPE_SHIFT:usize =			(56);
// const GITS_BASER_TYPE:usize =(r)		(((r) >> GITS_BASER_TYPE_SHIFT) & 7);
// const GITS_BASER_ENTRY_SIZE_SHIFT:usize =		(48);
// const GITS_BASER_ENTRY_SIZE:usize =(r)	((((r) >> GITS_BASER_ENTRY_SIZE_SHIFT) & 0x1f) + 1);
// const GITS_BASER_ENTRY_SIZE_MASK:usize =	GENMASK_ULL(52, 48);
// const GITS_BASER_PHYS_52_to_48:usize =(phys)					\;
// 	(((phys) & GENMASK_ULL(47, 16)) | (((phys) >> 48) & 0xf) << 12)
// const GITS_BASER_SHAREABILITY_SHIFT:usize =	(10);
// const GITS_BASER_InnerShareable:usize =					\;
// 	GIC_BASER_SHAREABILITY(GITS_BASER, InnerShareable)
// const GITS_BASER_PAGE_SIZE_SHIFT:usize =	(8);
// const GITS_BASER_PAGE_SIZE_4K:usize =		(0ULL << GITS_BASER_PAGE_SIZE_SHIFT);
// const GITS_BASER_PAGE_SIZE_16K:usize =	(1ULL << GITS_BASER_PAGE_SIZE_SHIFT);
// const GITS_BASER_PAGE_SIZE_64K:usize =	(2ULL << GITS_BASER_PAGE_SIZE_SHIFT);
// const GITS_BASER_PAGE_SIZE_MASK:usize =	(3ULL << GITS_BASER_PAGE_SIZE_SHIFT);
// const GITS_BASER_PAGES_MAX:usize =		256;
// const GITS_BASER_PAGES_SHIFT:usize =		(0);
// const GITS_BASER_NR_PAGES:usize =(r)		(((r) & 0xff) + 1);

// const GITS_BASER_TYPE_NONE:usize =		0;
// const GITS_BASER_TYPE_DEVICE:usize =		1;
// const GITS_BASER_TYPE_VCPU:usize =		2;
// const GITS_BASER_TYPE_RESERVED3:usize =	3;
// const GITS_BASER_TYPE_COLLECTION:usize =	4;
// const GITS_BASER_TYPE_RESERVED5:usize =	5;
// const GITS_BASER_TYPE_RESERVED6:usize =	6;
// const GITS_BASER_TYPE_RESERVED7:usize =	7;

// const GITS_LVL1_ENTRY_SIZE:usize =           (8UL);

/*
 * ITS commands
 */
// const GITS_CMD_MAPD:usize =			0x08;
// const GITS_CMD_MAPC:usize =			0x09;
// const GITS_CMD_MAPTI:usize =			0x0a;
// const GITS_CMD_MAPI:usize =			0x0b;
// const GITS_CMD_MOVI:usize =			0x01;
// const GITS_CMD_DISCARD:usize =		0x0f;
// const GITS_CMD_INV:usize =			0x0c;
// const GITS_CMD_MOVALL:usize =			0x0e;
// const GITS_CMD_INVALL:usize =			0x0d;
// const GITS_CMD_INT:usize =			0x03;
// const GITS_CMD_CLEAR:usize =			0x04;
// const GITS_CMD_SYNC:usize =			0x05;

/*
 * GICv4 ITS specific commands
 */
// const GITS_CMD_GICv4:usize =(x)		((x) | 0x20);
// const GITS_CMD_VINVALL:usize =		GITS_CMD_GICv4(GITS_CMD_INVALL);
// const GITS_CMD_VMAPP:usize =			GITS_CMD_GICv4(GITS_CMD_MAPC);
// const GITS_CMD_VMAPTI:usize =			GITS_CMD_GICv4(GITS_CMD_MAPTI);
// const GITS_CMD_VMOVI:usize =			GITS_CMD_GICv4(GITS_CMD_MOVI);
// const GITS_CMD_VSYNC:usize =			GITS_CMD_GICv4(GITS_CMD_SYNC);
// /* VMOVP is the odd one, as it doesn't have a physical counterpart */
// const GITS_CMD_VMOVP:usize =			GITS_CMD_GICv4(2);

/*
 * ITS error numbers
 */
// const E_ITS_MOVI_UNMAPPED_INTERRUPT:usize =		0x010107;
// const E_ITS_MOVI_UNMAPPED_COLLECTION:usize =		0x010109;
// const E_ITS_INT_UNMAPPED_INTERRUPT:usize =		0x010307;
// const E_ITS_CLEAR_UNMAPPED_INTERRUPT:usize =		0x010507;
// const E_ITS_MAPD_DEVICE_OOR:usize =			0x010801;
// const E_ITS_MAPD_ITTSIZE_OOR:usize =			0x010802;
// const E_ITS_MAPC_PROCNUM_OOR:usize =			0x010902;
// const E_ITS_MAPC_COLLECTION_OOR:usize =		0x010903;
// const E_ITS_MAPTI_UNMAPPED_DEVICE:usize =		0x010a04;
// const E_ITS_MAPTI_ID_OOR:usize =			0x010a05;
// const E_ITS_MAPTI_PHYSICALID_OOR:usize =		0x010a06;
// const E_ITS_INV_UNMAPPED_INTERRUPT:usize =		0x010c07;
// const E_ITS_INVALL_UNMAPPED_COLLECTION:usize =	0x010d09;
// const E_ITS_MOVALL_PROCNUM_OOR:usize =		0x010e01;
// const E_ITS_DISCARD_UNMAPPED_INTERRUPT:usize =	0x010f07;

/*
 * CPU interface registers
 */
const ICC_CTLR_EL1_EOIMODE_SHIFT: u32 = 1;
const ICC_CTLR_EL1_EOIMODE_DROP_DIR: u32 = 0 << ICC_CTLR_EL1_EOIMODE_SHIFT;
const ICC_CTLR_EL1_EOIMODE_DROP: u32 = 1 << ICC_CTLR_EL1_EOIMODE_SHIFT;
const ICC_CTLR_EL1_EOIMODE_MASK: u32 = 1 << ICC_CTLR_EL1_EOIMODE_SHIFT;
const ICC_CTLR_EL1_CBPR_SHIFT: u32 = 0;
const ICC_CTLR_EL1_CBPR_MASK: u32 = 1 << ICC_CTLR_EL1_CBPR_SHIFT;
const ICC_CTLR_EL1_PRI_BITS_SHIFT: u32 = 8;
const ICC_CTLR_EL1_PRI_BITS_MASK: u32 = 0x7 << ICC_CTLR_EL1_PRI_BITS_SHIFT;
const ICC_CTLR_EL1_ID_BITS_SHIFT: u32 = 11;
const ICC_CTLR_EL1_ID_BITS_MASK: u32 = 0x7 << ICC_CTLR_EL1_ID_BITS_SHIFT;
const ICC_CTLR_EL1_SEIS_SHIFT: u32 = 14;
const ICC_CTLR_EL1_SEIS_MASK: u32 = 0x1 << ICC_CTLR_EL1_SEIS_SHIFT;
const ICC_CTLR_EL1_A3V_SHIFT: u32 = 15;
const ICC_CTLR_EL1_A3V_MASK: u32 = 0x1 << ICC_CTLR_EL1_A3V_SHIFT;
const ICC_CTLR_EL1_RSS: u32 = 0x1 << 18;
const ICC_PMR_EL1_SHIFT: u32 = 0;
const ICC_PMR_EL1_MASK: u32 = 0xff << ICC_PMR_EL1_SHIFT;
const ICC_BPR0_EL1_SHIFT: u32 = 0;
const ICC_BPR0_EL1_MASK: u32 = 0x7 << ICC_BPR0_EL1_SHIFT;
const ICC_BPR1_EL1_SHIFT: u32 = 0;
const ICC_BPR1_EL1_MASK: u32 = 0x7 << ICC_BPR1_EL1_SHIFT;
const ICC_IGRPEN0_EL1_SHIFT: u32 = 0;
const ICC_IGRPEN0_EL1_MASK: u32 = 1 << ICC_IGRPEN0_EL1_SHIFT;
const ICC_IGRPEN1_EL1_SHIFT: u32 = 0;
const ICC_IGRPEN1_EL1_MASK: u32 = 1 << ICC_IGRPEN1_EL1_SHIFT;
const ICC_SRE_EL1_DIB: u32 = 1 << 2;
const ICC_SRE_EL1_DFB: u32 = 1 << 1;
const ICC_SRE_EL1_SRE: u32 = 1 << 0;

/*
 * Hypervisor interface registers (SRE only)
 */
// const ICH_LR_VIRTUAL_ID_MASK:usize =		((1ULL << 32) - 1);

// const ICH_LR_EOI:usize =			(1ULL << 41);
// const ICH_LR_GROUP:usize =			(1ULL << 60);
// const ICH_LR_HW:usize =			(1ULL << 61);
// const ICH_LR_STATE:usize =			(3ULL << 62);
// const ICH_LR_PENDING_BIT:usize =		(1ULL << 62);
// const ICH_LR_ACTIVE_BIT:usize =		(1ULL << 63);
// const ICH_LR_PHYS_ID_SHIFT:usize =		32;
// const ICH_LR_PHYS_ID_MASK:usize =		(0x3ffULL << ICH_LR_PHYS_ID_SHIFT);
// const ICH_LR_PRIORITY_SHIFT:usize =		48;
// const ICH_LR_PRIORITY_MASK:usize =		(0xffULL << ICH_LR_PRIORITY_SHIFT);

// /* These are for GICv2 emulation only */
// const GICH_LR_VIRTUALID:usize =		(0x3ffUL << 0);
// const GICH_LR_PHYSID_CPUID_SHIFT:usize =	(10);
// const GICH_LR_PHYSID_CPUID:usize =		(7UL << GICH_LR_PHYSID_CPUID_SHIFT);

// const ICH_MISR_EOI:usize =			(1 << 0);
// const ICH_MISR_U:usize =			(1 << 1);

// const ICH_HCR_EN:usize =			(1 << 0);
// const ICH_HCR_UIE:usize =			(1 << 1);
// const ICH_HCR_NPIE:usize =			(1 << 3);
// const ICH_HCR_TC:usize =			(1 << 10);
// const ICH_HCR_TALL0:usize =			(1 << 11);
// const ICH_HCR_TALL1:usize =			(1 << 12);
// const ICH_HCR_EOIcount_SHIFT:usize =		27;
// const ICH_HCR_EOIcount_MASK:usize =		(0x1f << ICH_HCR_EOIcount_SHIFT);

// const ICH_VMCR_ACK_CTL_SHIFT:usize =		2;
// const ICH_VMCR_ACK_CTL_MASK:usize =		(1 << ICH_VMCR_ACK_CTL_SHIFT);
// const ICH_VMCR_FIQ_EN_SHIFT:usize =		3;
// const ICH_VMCR_FIQ_EN_MASK:usize =		(1 << ICH_VMCR_FIQ_EN_SHIFT);
// const ICH_VMCR_CBPR_SHIFT:usize =		4;
// const ICH_VMCR_CBPR_MASK:usize =		(1 << ICH_VMCR_CBPR_SHIFT);
// const ICH_VMCR_EOIM_SHIFT:usize =		9;
// const ICH_VMCR_EOIM_MASK:usize =		(1 << ICH_VMCR_EOIM_SHIFT);
// const ICH_VMCR_BPR1_SHIFT:usize =		18;
// const ICH_VMCR_BPR1_MASK:usize =		(7 << ICH_VMCR_BPR1_SHIFT);
// const ICH_VMCR_BPR0_SHIFT:usize =		21;
// const ICH_VMCR_BPR0_MASK:usize =		(7 << ICH_VMCR_BPR0_SHIFT);
// const ICH_VMCR_PMR_SHIFT:usize =		24;
// const ICH_VMCR_PMR_MASK:usize =		(0xffUL << ICH_VMCR_PMR_SHIFT);
// const ICH_VMCR_ENG0_SHIFT:usize =		0;
// const ICH_VMCR_ENG0_MASK:usize =		(1 << ICH_VMCR_ENG0_SHIFT);
// const ICH_VMCR_ENG1_SHIFT:usize =		1;
// const ICH_VMCR_ENG1_MASK:usize =		(1 << ICH_VMCR_ENG1_SHIFT);

// const ICH_VTR_PRI_BITS_SHIFT:usize =		29;
// const ICH_VTR_PRI_BITS_MASK:usize =		(7 << ICH_VTR_PRI_BITS_SHIFT);
// const ICH_VTR_ID_BITS_SHIFT:usize =		23;
// const ICH_VTR_ID_BITS_MASK:usize =		(7 << ICH_VTR_ID_BITS_SHIFT);
// const ICH_VTR_SEIS_SHIFT:usize =		22;
// const ICH_VTR_SEIS_MASK:usize =		(1 << ICH_VTR_SEIS_SHIFT);
// const ICH_VTR_A3V_SHIFT:usize =		21;
// const ICH_VTR_A3V_MASK:usize =		(1 << ICH_VTR_A3V_SHIFT);

const ICC_IAR1_EL1_SPURIOUS: u32 = 0x3ff;

const ICC_SRE_EL2_SRE: u32 = 1 << 0;
const ICC_SRE_EL2_ENABLE: u32 = 1 << 3;

const ICC_SGI1R_TARGET_LIST_SHIFT: u32 = 0;
const ICC_SGI1R_TARGET_LIST_MASK: u32 = 0xffff << ICC_SGI1R_TARGET_LIST_SHIFT;
const ICC_SGI1R_AFFINITY_1_SHIFT: u32 = 16;
const ICC_SGI1R_AFFINITY_1_MASK: u32 = 0xff << ICC_SGI1R_AFFINITY_1_SHIFT;
const ICC_SGI1R_SGI_ID_SHIFT: u32 = 24;
const ICC_SGI1R_SGI_ID_MASK: u64 = 0xf << ICC_SGI1R_SGI_ID_SHIFT;
const ICC_SGI1R_AFFINITY_2_SHIFT: u32 = 32;
const ICC_SGI1R_AFFINITY_2_MASK: u64 = 0xff << ICC_SGI1R_AFFINITY_2_SHIFT;
const ICC_SGI1R_IRQ_ROUTING_MODE_BIT: u32 = 40;
const ICC_SGI1R_RS_SHIFT: u32 = 44;
const ICC_SGI1R_RS_MASK: u64 = 0xf << ICC_SGI1R_RS_SHIFT;
const ICC_SGI1R_AFFINITY_3_SHIFT: u32 = 48;
const ICC_SGI1R_AFFINITY_3_MASK: u64 = 0xff << ICC_SGI1R_AFFINITY_3_SHIFT;

const GIC_CPU_CTRL: u32 = 0x00;
const GIC_CPU_PRIMASK: u32 = 0x04;
const GIC_CPU_BINPOINT: u32 = 0x08;
const GIC_CPU_INTACK: u32 = 0x0c;
const GIC_CPU_EOI: u32 = 0x10;
const GIC_CPU_RUNNINGPRI: u32 = 0x14;
const GIC_CPU_HIGHPRI: u32 = 0x18;
const GIC_CPU_ALIAS_BINPOINT: u32 = 0x1c;
const GIC_CPU_ACTIVEPRIO: u32 = 0xd0;
const GIC_CPU_IDENT: u32 = 0xfc;
const GIC_CPU_DEACTIVATE: u32 = 0x1000;

const GICC_ENABLE: u32 = 0x1;
const GICC_INT_PRI_THRESHOLD: u32 = 0xf0;

const GIC_CPU_CTRL_ENABLE_GRP0_SHIFT: u32 = 0;
const GIC_CPU_CTRL_ENABLE_GRP0: u32 = 1 << GIC_CPU_CTRL_ENABLE_GRP0_SHIFT;
const GIC_CPU_CTRL_ENABLE_GRP1_SHIFT: u32 = 1;
const GIC_CPU_CTRL_ENABLE_GRP1: u32 = 1 << GIC_CPU_CTRL_ENABLE_GRP1_SHIFT;
const GIC_CPU_CTRL_ACKCTL_SHIFT: u32 = 2;
const GIC_CPU_CTRL_ACKCTL: u32 = 1 << GIC_CPU_CTRL_ACKCTL_SHIFT;
const GIC_CPU_CTRL_FIQ_EN_SHIFT: u32 = 3;
const GIC_CPU_CTRL_FIQ_EN: u32 = 1 << GIC_CPU_CTRL_FIQ_EN_SHIFT;
const GIC_CPU_CTRL_CBPR_SHIFT: u32 = 4;
const GIC_CPU_CTRL_CBPR: u32 = 1 << GIC_CPU_CTRL_CBPR_SHIFT;
const GIC_CPU_CTRL_EOIMODE_NS_SHIFT: u32 = 9;
const GIC_CPU_CTRL_EOIMODE_NS: u32 = 1 << GIC_CPU_CTRL_EOIMODE_NS_SHIFT;

const GICC_IAR_INT_ID_MASK: u32 = 0x3ff;
const GICC_INT_SPURIOUS: u32 = 1023;
const GICC_DIS_BYPASS_MASK: u32 = 0x1e0;

const GIC_DIST_CTRL: usize = 0x000;
const GIC_DIST_CTR: usize = 0x004;
const GIC_DIST_IIDR: usize = 0x008;
const GIC_DIST_IGROUP: usize = 0x080;
const GIC_DIST_ENABLE_SET: usize = 0x100;
const GIC_DIST_ENABLE_CLEAR: usize = 0x180;
const GIC_DIST_PENDING_SET: usize = 0x200;
const GIC_DIST_PENDING_CLEAR: usize = 0x280;
const GIC_DIST_ACTIVE_SET: usize = 0x300;
const GIC_DIST_ACTIVE_CLEAR: usize = 0x380;
const GIC_DIST_PRI: usize = 0x400;
const GIC_DIST_TARGET: usize = 0x800;
const GIC_DIST_CONFIG: usize = 0xc00;
const GIC_DIST_SOFTINT: usize = 0xf00;
const GIC_DIST_SGI_PENDING_CLEAR: usize = 0xf10;
const GIC_DIST_SGI_PENDING_SET: usize = 0xf20;

const GICD_ENABLE: usize = 0x1;
const GICD_DISABLE: usize = 0x0;
const GICD_INT_ACTLOW_LVLTRIG: u32 = 0x0;
const GICD_INT_EN_CLR_X32: u32 = 0xffffffff;
const GICD_INT_EN_SET_SGI: u32 = 0x0000ffff;
const GICD_INT_EN_CLR_PPI: u32 = 0xffff0000;
const GICD_INT_DEF_PRI: u32 = 0xa0;
const GICD_INT_DEF_PRI_X4: u32 =
    GICD_INT_DEF_PRI << 24 | GICD_INT_DEF_PRI << 16 | GICD_INT_DEF_PRI << 8 | GICD_INT_DEF_PRI;

const GICH_HCR: u32 = 0x0;
const GICH_VTR: u32 = 0x4;
const GICH_VMCR: u32 = 0x8;
const GICH_MISR: u32 = 0x10;
const GICH_EISR0: u32 = 0x20;
const GICH_EISR1: u32 = 0x24;
const GICH_ELRSR0: u32 = 0x30;
const GICH_ELRSR1: u32 = 0x34;
const GICH_APR: u32 = 0xf0;
const GICH_LR0: u32 = 0x100;

const GICH_HCR_EN: u32 = 1 << 0;
const GICH_HCR_UIE: u32 = 1 << 1;
const GICH_HCR_NPIE: u32 = 1 << 3;

const GICH_LR_VIRTUALID: u32 = 0x3ff << 0;
const GICH_LR_PHYSID_CPUID_SHIFT: u32 = 10;
const GICH_LR_PHYSID_CPUID: u32 = 0x3ff << GICH_LR_PHYSID_CPUID_SHIFT;
const GICH_LR_PRIORITY_SHIFT: u32 = 23;
const GICH_LR_STATE: u32 = 3 << 28;
const GICH_LR_PENDING_BIT: u32 = 1 << 28;
const GICH_LR_ACTIVE_BIT: u32 = 1 << 29;
const GICH_LR_EOI: u32 = 1 << 19;
const GICH_LR_GROUP1: u32 = 1 << 30;
const GICH_LR_HW: u32 = 1 << 31;

const GICH_VMCR_ENABLE_GRP0_SHIFT: u32 = 0;
const GICH_VMCR_ENABLE_GRP0_MASK: u32 = 1 << GICH_VMCR_ENABLE_GRP0_SHIFT;
const GICH_VMCR_ENABLE_GRP1_SHIFT: u32 = 1;
const GICH_VMCR_ENABLE_GRP1_MASK: u32 = 1 << GICH_VMCR_ENABLE_GRP1_SHIFT;
const GICH_VMCR_ACK_CTL_SHIFT: u32 = 2;
const GICH_VMCR_ACK_CTL_MASK: u32 = 1 << GICH_VMCR_ACK_CTL_SHIFT;
const GICH_VMCR_FIQ_EN_SHIFT: u32 = 3;
const GICH_VMCR_FIQ_EN_MASK: u32 = 1 << GICH_VMCR_FIQ_EN_SHIFT;
const GICH_VMCR_CBPR_SHIFT: u32 = 4;
const GICH_VMCR_CBPR_MASK: u32 = 1 << GICH_VMCR_CBPR_SHIFT;
const GICH_VMCR_EOI_MODE_SHIFT: u32 = 9;
const GICH_VMCR_EOI_MODE_MASK: u32 = 1 << GICH_VMCR_EOI_MODE_SHIFT;

const GICH_VMCR_PRIMASK_SHIFT: u32 = 27;
const GICH_VMCR_PRIMASK_MASK: u32 = 0x1f << GICH_VMCR_PRIMASK_SHIFT;
const GICH_VMCR_BINPOINT_SHIFT: u32 = 21;
const GICH_VMCR_BINPOINT_MASK: u32 = 0x7 << GICH_VMCR_BINPOINT_SHIFT;
const GICH_VMCR_ALIAS_BINPOINT_SHIFT: u32 = 18;
const GICH_VMCR_ALIAS_BINPOINT_MASK: u32 = 0x7 << GICH_VMCR_ALIAS_BINPOINT_SHIFT;

const GICH_MISR_EOI: u32 = 1 << 0;
const GICH_MISR_U: u32 = 1 << 1;

const GICV_PMR_PRIORITY_SHIFT: u32 = 3;
const GICV_PMR_PRIORITY_MASK: u32 = 0x1f << GICV_PMR_PRIORITY_SHIFT;

const MPIDR_HWID_BITMASK: u64 = 0xff00ffffff;

// register_structs! {
//   #[allownon_snake_case]
//   GicDistributorBlock {
//     (0x0000 => CTLR: ReadWrite<u32>,
//     (0x0004 => TYPER: ReadOnly<u32>),
//     (0x0008 => IIDR: ReadOnly<u32>),
//     (0x000c => _reserved_0),
//     (0x0010 => STATUSR: ReadOnly<u32>),
//     (0x0014 => _reserved_1),
//     (0x0080 => IGROUPR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0100 => ISENABLER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0180 => ICENABLER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0200 => ISPENDR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0280 => ICPENDR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0300 => ISACTIVER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0380 => ICACTIVER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
//     (0x0400 => IPRIORITYR: [ReadWrite<u32>; GIC_8_BIT_NUM]),
//     (0x0800 => ITARGETSR: [ReadWrite<u32>; GIC_8_BIT_NUM]),
//     (0x0c00 => ICFGR: [ReadWrite<u32>; GIC_2_BIT_NUM]),
//     (0x0d00 => _reserved_2),
//     (0x0e00 => NSACR: [ReadWrite<u32>; GIC_2_BIT_NUM]),
//     (0x0f00 => SGIR: WriteOnly<u32>),
//     (0x0f04 => _reserved_3),
//     (0x0f10 => CPENDSGIR: [ReadWrite<u32>; GIC_SGI_NUM * 8 / 32]),
//     (0x0f20 => SPENDSGIR: [ReadWrite<u32>; GIC_SGI_NUM * 8 / 32]),
//     (0x0f30 => _reserved_4),
//     (0x6000 => IROUTER: [ReadWrite<u64>; 1020]),
//     (0x7fd8 => _reserved_5),
//     (0xFFE8 => PIDR2: ReadOnly<u32>),
//     (0xFFF0 => @END),
//   }
// }

#[derive(Debug)]
struct GicDistributor {
    base_addr: usize,
    gic_irqs: u32,
}

// register_structs! {
//   #[allow(non_snake_case)]
//   GicCpuInterfaceBlock {
//     (0x0000 => CTLR: ReadWrite<u32>),   // CPU Interface Control Register
//     (0x0004 => PMR: ReadWrite<u32>),    // Interrupt Priority Mask Register
//     (0x0008 => BPR: ReadWrite<u32>),    // Binary Point Register
//     (0x000c => IAR: ReadOnly<u32>),     // Interrupt Acknowledge Register
//     (0x0010 => EOIR: WriteOnly<u32>),   // End of Interrupt Register
//     (0x0014 => RPR: ReadOnly<u32>),     // Running Priority Register
//     (0x0018 => HPPIR: ReadOnly<u32>),   // Highest Priority Pending Interrupt Register
//     (0x001c => ABPR: ReadWrite<u32>),   // Aliased Binary Point Register
//     (0x0020 => AIAR: ReadOnly<u32>),    // Aliased Interrupt Acknowledge Register
//     (0x0024 => AEOIR: WriteOnly<u32>),  // Aliased End of Interrupt Register
//     (0x0028 => AHPPIR: ReadOnly<u32>),  // Aliased Highest Priority Pending Interrupt Register
//     (0x002c => _reserved_0),
//     (0x00d0 => APR: [ReadWrite<u32>; 4]),    // Active Priorities Register
//     (0x00e0 => NSAPR: [ReadWrite<u32>; 4]),  // Non-secure Active Priorities Register
//     (0x00f0 => _reserved_1),
//     (0x00fc => IIDR: ReadOnly<u32>),    // CPU Interface Identification Register
//     (0x0100 => _reserved_2),
//     (0x1000 => DIR: WriteOnly<u32>),    // Deactivate Interrupt Register
//     (0x1004 => _reserved_3),
//     (0x2000 => @END),
//   }
// }

#[derive(Debug)]
struct GicCpuInterface {
    base_addr: usize,
    gic_irqs: u32,
    redists: [usize; NCPU],
}

const MPIDR_LEVEL_BITS_SHIFT: u32 = 3;
const MPIDR_LEVEL_BITS: u32 = 1 << MPIDR_LEVEL_BITS_SHIFT;
const MPIDR_LEVEL_MASK: u32 = (1 << MPIDR_LEVEL_BITS) - 1;

#[derive(Debug, Copy, Clone)]
struct GicRedistributor {
    base_addr: usize,
    gic_irqs: u32,
}

impl GicRedistributor {
    pub const fn new() -> Self {
        GicRedistributor {
            base_addr: 0,
            gic_irqs: 0,
        }
    }

    pub fn from_addr(addr: usize) -> Self {
        GicRedistributor {
            base_addr: addr,
            gic_irqs: 0,
        }
    }

    pub fn set_base_addr(&mut self, addr: usize) {
        self.base_addr = addr;
    }
}

impl GicCpuInterface {
    const fn new(base_addr: usize) -> Self {
        GicCpuInterface {
            base_addr,
            gic_irqs: 0,
            redists: [0usize; NCPU],
        }
    }

    fn init(&self) {
        self.enable_redist();

        let rbase = self.rdist_sgi_base();

        /* Configure SGIs/PPIs as non-secure Group-1 */
        unsafe {
            ptr::write_volatile((rbase + GICR_IGROUPR0) as *mut u32, u32::MAX);
            isb();
        }

        self.cpu_config(rbase);

        self.sys_reg_init();

        isb();
    }

    fn cpu_config(&self, base: usize) {
        unsafe {
            ptr::write_volatile(
                (base + GIC_DIST_ACTIVE_CLEAR) as *mut u32,
                GICD_INT_EN_CLR_X32,
            );
            ptr::write_volatile(
                (base + GIC_DIST_ENABLE_CLEAR) as *mut u32,
                GICD_INT_EN_CLR_PPI,
            );
            ptr::write_volatile(
                (base + GIC_DIST_ENABLE_SET) as *mut u32,
                GICD_INT_EN_SET_SGI,
            );
        }
        isb();
        /*
         * Set priority on PPI and SGI interrupts
         */
        for i in (0..32).step_by(4) {
            unsafe {
                ptr::write_volatile(
                    (base + GIC_DIST_PRI + i * 4 / 4) as *mut u32,
                    GICD_INT_DEF_PRI_X4,
                );
            }
        }
        isb();

        self.redist_wait_for_rwp();

        self.sys_reg_init();
    }

    fn redist_wait_for_rwp(&self) {
        wait_for_rwp(self.data_rdist_rd_base());
    }

    fn enable_redist(&self) {
        let mut count = 10000000; // 1s

        let rbase = self.data_rdist_rd_base();

        let mut val = unsafe { read_w(rbase + GICR_WAKER) };

        val &= !(GICR_WAKER_PROC_SLEEP as u32);
        unsafe { write_w(rbase + GICR_WAKER, val) };

        while count > 1 {
            count -= 1;
            let val = unsafe { read_w(rbase + GICR_WAKER) };

            if val & GICR_WAKER_CHILDREN_ASLEEP as u32 == 0 {
                break;
            }
            cpu_relax();
            Timer::udelay(1);
        }
        if count == 1 {
            panic!("redistributor failed to wakeup")
        }
    }

    fn data_rdist_rd_base(&self) -> usize {
        let mpidr: u64 = r_mpidr() as u64 & MPIDR_HWID_BITMASK;
        // let typer:u64;
        let aff: u32 = mpidr_affinity_level(mpidr, 3) << 24
            | mpidr_affinity_level(mpidr, 2) << 16
            | mpidr_affinity_level(mpidr, 1) << 8
            | mpidr_affinity_level(mpidr, 0);

        for i in 0usize..self.gic_irqs as usize {
            let typer: u64 = unsafe { read_d(self.redists[i] + GICR_TYPER) };

            if (typer >> 32) == aff as u64 {
                return self.redists[i];
            }
        }

        // TODO: Is this safe? originally NULL in c.
        panic!("no rd base")
    }

    fn rdist_sgi_base(&self) -> usize {
        self.data_rdist_rd_base() + 0x10000
    }

    fn irq_in_rdist(hwirq: usize) -> bool {
        hwirq < 32
    }

    fn sys_reg_init(&self) {
        let mut pribits = r_icc_ctlr_el1();
        pribits &= ICC_CTLR_EL1_PRI_BITS_MASK;
        pribits >>= ICC_CTLR_EL1_PRI_BITS_SHIFT;
        pribits += 1;

        let x: usize = 1 << (8 - pribits);
        unsafe {
            asm!("msr icc_pmr_el1, {}", in(reg) x);
            isb();
        }
        let val = unsafe {
            let mut x: usize;
            asm!("mrs {}, icc_pmr_el1", out(reg) x);
            x
        };

        // set priority mask register
        unsafe {
            let x: usize = 0xf0;
            asm!("msr icc_pmr_el1, {}", in(reg) x);
            isb();
            /*
             * Some firmwares hand over to the kernel with the BPR changed from
             * its reset value (and with a value large enough to prevent
             * any pre-emptive interrupts from working at all). Writing a zero
             * to BPR restores is reset value.
             */
            asm!("msr icc_bpr1_el1, xzr");
            isb();

            let x: usize = ICC_CTLR_EL1_EOIMODE_DROP_DIR as usize;
            asm!("msr icc_ctlr_el1, {}", in(reg) x);
            isb();
        }

        if pribits < 8 {
            if val != 0 {
                // group 0
                if pribits > 6 {
                    unsafe {
                        asm!("msr icc_ap0r3_el1, xzr");
                        asm!("msr icc_ap0r2_el1, xzr");
                    }
                }
                if pribits > 5 {
                    unsafe {
                        asm!("msr icc_ap0r1_el1, xzr");
                    }
                }
                if pribits > 3 {
                    unsafe {
                        asm!("msr icc_ap0r0_el1, xzr");
                    }
                }

                isb();
            }

            if pribits > 6 {
                unsafe {
                    asm!("msr icc_ap1r3_el1, xzr");
                    asm!("msr icc_ap1r2_el1, xzr");
                }
            }
            if pribits > 5 {
                unsafe {
                    asm!("msr icc_ap1r1_el1, xzr");
                }
            }
            if pribits > 3 {
                unsafe {
                    asm!("msr icc_ap1r0_el1, xzr");
                }
            }

            isb();
        }

        unsafe {
            let x: usize = 1;
            asm!("msr icc_igrpen1_el1, {}", in(reg) x);
        }
        isb();
    }
}

impl GicDistributor {
    const fn new(base_addr: usize) -> Self {
        GicDistributor {
            base_addr,
            gic_irqs: 0,
        }
    }

    fn dist_wait_for_rwp(&self) {
        wait_for_rwp(self.base_addr);
    }

    fn init(&self) {
        // Disable the distributor
        unsafe { write_w(self.base_addr + GICD_CTLR, 0) };
        self.dist_wait_for_rwp();

        /*
         * Configure SPIs as non-secure Group-1. This will only matter
         * if the GIC only has a single security state. This will not
         * do the right thing if the kernel is running in secure mode,
         * but that's not the intended use case anyway.
         */
        for i in (32..self.gic_irqs as usize).step_by(32) {
            unsafe { write_w(self.base_addr + GICD_IGROUPR + i / 8, !0u32) };
        }
        isb();

        dist_config(self.base_addr, self.gic_irqs, || {
            self.dist_wait_for_rwp();
        });

        unsafe {
            // enable distributor with ARE, group1
            write_w(
                self.base_addr + GICD_CTLR,
                GICD_CTLR_ARE_NS | GICD_CTLR_ENABLE_G1A | GICD_CTLR_ENABLE_G1,
            );
        }

        /*
         * Set all global interrupts to the boot CPU only. ARE must be
         * enabled.
         */
        let mpidr = r_mpidr() as u64;
        let affinity = mpidr_to_affinity(mpidr);

        for i in 32..self.gic_irqs as usize {
            unsafe { write_d(self.base_addr + GICD_IROUTER + i * 8, affinity) };
        }
        isb();
    }

    fn init_per_core(&self) {
        // TODO: nothing to do
    }

    #[no_mangle]
    pub fn validate_gic_version(&self) {
        let version = unsafe { ptr::read_volatile((self.base_addr + GICD_PIDR2) as *mut u32) };
        let version = version & GIC_PIDR2_ARCH_MASK;
        // let version = self.PIDR2.get() & GIC_PIDR2_ARCH_MASK;

        if version != GIC_PIDR2_ARCH_GICV3 && version != GIC_PIDR2_ARCH_GICV4 {
            panic!("unsupported gic version")
        }
    }
}

static GICD: GicDistributor = GicDistributor::new(GICD_BASE);
static GICC: GicCpuInterface = GicCpuInterface::new(GICC_BASE);

#[derive(Debug)]
pub struct Gic {
    gicc: GicCpuInterface,
    gicd: GicDistributor,
}

impl Gic {
    pub const fn new(gicd_base: usize, gicc_base: usize) -> Self {
        Self {
            gicc: GicCpuInterface::new(gicc_base),
            gicd: GicDistributor::new(gicd_base),
        }
    }

    pub fn init(&mut self) {
        // check gic version is matched: should be v3 or 4
        self.gicd.validate_gic_version();

        let mut rdist_base = GICR_BASE;
        for i in 0..NCPU {
            self.gicc.redists[i] = rdist_base;
            rdist_base += 0x20000;
        }

        let typer = unsafe { read_w(self.gicd.base_addr + GICD_TYPER) };

        let gic_irqs = ((typer & 0x1f) + 1) * 32;

        let gic_irqs = if gic_irqs > 1020 { 1020 } else { gic_irqs };

        self.gicd.gic_irqs = gic_irqs;
        self.gicc.gic_irqs = gic_irqs;

        if cpu_id() == 0 {
            self.gicd.init();
        }
        self.gicc.init();
    }

    pub fn init_core(&self) {}

    fn poke_irq(&self, hwirq: u32, offset: u32) {
        let mask: u32 = 1 << (hwirq % 32);

        if irq_in_rdist(hwirq) {
            let base = self.gicc.rdist_sgi_base();
            unsafe {
                *((base + offset as usize + (hwirq as usize / 32) * 4) as *mut u32) = mask;
            }
            self.gicc.redist_wait_for_rwp();
        } else {
            let base = self.gicd.base_addr;
            unsafe {
                *((base + offset as usize + (hwirq as usize / 32) * 4) as *mut u32) = mask;
            }
            self.gicd.dist_wait_for_rwp();
        }
    }

    pub fn enable(&self, int: Interrupt) {
        self.poke_irq(int as u32, 0x0100);
    }

    // pub fn disable(&self, _int: Interrupt) {
    //     // TODO
    //     // let gicd = &GICD;
    //     // gicd.clear_enable(int);
    // }

    pub fn fetch(&self) -> Option<Interrupt> {
        let mut x;
        unsafe {
            asm!("mrs {}, icc_iar1_el1", out(reg) x);
            asm!("dsb sy");
        }
        Some(x)
    }

    pub fn finish(&self, int: Interrupt) {
        unsafe {
            let x = int;
            asm!("msr icc_eoir1_el1, {}", in(reg) x);
        }
    }
}

pub const INT_TIMER: Interrupt = 27; // virtual timer

pub static INTERRUPT_CONTROLLER: Gic = Gic::new(GICD_BASE, GICC_BASE);

pub type Interrupt = usize;

pub unsafe fn intr_init() {}

pub unsafe fn intr_init_core() {
    DAIF.set(DAIF::I::Masked.into());

    let mut intr_controller = Gic::new(GICD_BASE, GICC_BASE);
    intr_controller.init();

    Timer::init();
    intr_controller.enable(TIMER0_IRQ);

    // Order matters!
    if cpu_id() == 0 {
        // only boot core do this initialization

        // virtio_blk
        intr_controller.enable(MemLayout::VIRTIO0_IRQ);

        // pl011 uart
        intr_controller.enable(MemLayout::UART0_IRQ);
    }
}

fn mpidr_level_shift(level: u32) -> u32 {
    ((1 << level) >> 1) << MPIDR_LEVEL_BITS_SHIFT
}

fn mpidr_affinity_level(mpidr: u64, level: u32) -> u32 {
    (mpidr >> mpidr_level_shift(level)) as u32 & MPIDR_LEVEL_MASK
}

fn mpidr_to_affinity(mpidr: u64) -> u64 {
    (mpidr_affinity_level(mpidr, 2) as u64) << 16
        | (mpidr_affinity_level(mpidr, 1) as u64) << 8
        | (mpidr_affinity_level(mpidr, 0) as u64)
}

fn irq_in_rdist(hwirq: u32) -> bool {
    hwirq < 32
}

fn wait_for_rwp(base: usize) {
    let mut count = 1000000; /* 1s! */

    while unsafe { read_w(base + GICD_CTLR) } & GICD_CTLR_RWP != 0 {
        count -= 1;
        if count == 0 {
            panic!("RWP timeout, gone fishing");
        }
        cpu_relax();
        Timer::udelay(1);
    }
}

// read a word form the address
unsafe fn read_w(addr: usize) -> u32 {
    unsafe { ptr::read_volatile(addr as *mut u32) }
}

unsafe fn read_d(addr: usize) -> u64 {
    unsafe { ptr::read_volatile(addr as *mut u64) }
}

unsafe fn write_w(addr: usize, a: u32) {
    unsafe {
        ptr::write_volatile(addr as *mut u32, a);
        isb();
    }
}

unsafe fn write_d(addr: usize, a: u64) {
    unsafe {
        ptr::write_volatile(addr as *mut u64, a);
        isb();
    }
}

fn dist_config<F>(base: usize, gic_irqs: u32, sync_func: F)
where
    F: Fn(),
{
    /*
     * Set all global interrupts to be level triggered, active low.
     */
    for i in (32..gic_irqs as usize).step_by(16) {
        unsafe { write_w(base + GIC_DIST_CONFIG + i / 4, GICD_INT_ACTLOW_LVLTRIG) };
    }

    /*
     * Set priority on all global interrupts.
     */
    for i in (32..gic_irqs as usize).step_by(4) {
        unsafe { write_w(base + GIC_DIST_PRI + i, GICD_INT_DEF_PRI_X4) };
    }

    for i in (32..gic_irqs as usize).step_by(32) {
        unsafe {
            write_w(base + GIC_DIST_ACTIVE_CLEAR + i / 8, GICD_INT_EN_CLR_X32);
            write_w(base + GIC_DIST_ENABLE_CLEAR + i / 8, GICD_INT_EN_CLR_X32);
        };
    }
    sync_func();
}
