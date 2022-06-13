// On-disk file system format for the lfs.
// Both the kernel and user programs use this header file.

#include "kernel/types.h"

#define ROOTINO  1  // root i-number
#define BSIZE 1024  // block size

// Disk layout:
// [ boot block | super block | checkpoint1  | checkpoint2 |
//                                          segment summary, inode blocks, data blocks, and inode map ]
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

/// Block types. Used in segment summary entries.
#define SEGSUM_EMPTY    0
#define SEGSUM_INODE    1
#define SEGSUM_DATA     2
#define SEGSUM_INDIRECT 3
#define SEGSUM_IMAP     4

/// A single segment summary entry.
struct dsegsumentry {
  uint block_type; /// 0: empty, 1: inode, 2: data block, 3: indirect map, 4: imap block
  uint inum; // 0 in case of empty or imap block
  uint block_no; // 0 in case of inode or indirect map
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

