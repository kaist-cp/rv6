// On-disk file system format for the lfs..
// Both the kernel and user programs use this header file.

#include "kernel/types.h"

#define ROOTINO  1  // root i-number
#define BSIZE 1024  // block size
#define SEGSIZE 10  // segment size in blocks
#define FSSIZE 5000 // size of file system in blocks

// assumes inum : 0 ~ NINODES - 1
#define NINODES 200
#define NMETA 4
#define NSEG ((FSSIZE - NMETA) / SEGSIZE)
#define NINODEMAP ((NINODES * sizeof(uint) + BSIZE - 1) / BSIZE)

// Disk layout:
// [ boot block | super block | checkpoint1  | checkpoint2 |
//                                          inode map, inode blocks, and data blocks ]
//
// mklfs computes the super block and builds an initial file system. The
// super block describes the disk layout:
struct superblock {
  uint magic;        // Must be FSMAGIC
  uint size;         // Size of file system image (blocks)
  uint nblocks;      // Number of data blocks
  uint nsegments;    // Number of segments
  uint ninodes;      // Number of inodes
  uint checkpoint1;  // Block number of first checkpoint block
  uint checkpoint2;  // Block number of second checkpoint block
  uint segstart;     // Block number of first segment
};

// The size of the segment usage table in bytes. Always a multiple of 4.
#define SEGTABLESIZE ((NSEG + (sizeof(uint) * 8 - 1)) / (sizeof(uint) * 8) * 4)

struct checkpoint {
  uint imap[NINODEMAP];
  uchar segtable[SEGTABLESIZE]; // bitmap
  uint timestamp;
};

// Number of entries in each on-disk imap block.
#define NENTRY (BSIZE / sizeof(uint))

// A part of the imap stored in a single disk block.
// The actual imap may be stored in more than one block.
struct dimap {
  uint addr[NENTRY];
};

#define FSMAGIC 0x10203040

#define NDIRECT 12
#define NINDIRECT (BSIZE / sizeof(uint))
#define MAXFILE (NDIRECT + NINDIRECT)

// On-disk inode structure
struct dinode {
  short type;           // File type
  ushort major;         // Major device number (T_DEVICE only)
  ushort minor;         // Minor device number (T_DEVICE only)
  short nlink;          // Number of links to inode in file system
  uint size;            // Size of file (bytes)
  uint addrs[NDIRECT+1];   // Data block addresses
};

// Block containing inode i
#define IBLOCK(i, imp)     (imp[i])

// Directory is a file containing a sequence of dirent structures.
#define DIRSIZ 14

struct dirent {
  ushort inum;
  char name[DIRSIZ];
};

