#include <stdio.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <assert.h>

#define stat xv6_stat  // avoid clash with host struct stat
#include "kernel/types.h"
#include "lfs.h"
#include "kernel/stat.h"

#ifndef static_assert
#define static_assert(a, b) do { switch (0) case 0: case (a): ; } while (0)
#endif

// Disk layout:
// [ boot block | sb block | checkpoint1 (contains address of inode map blocks) | checkpoint2 (empty) | inode map | inode blocks and data blocks ]

int nblocks = FSSIZE - NMETA;  // Number of data blocks (imap, inode, and inode data blocks)

int fsfd;
struct superblock sb;
uint imp[NINODES]; //imap
char zeroes[BSIZE];
uint freeinode = 1;
uint freeblock;


void balloc(int);
void winode(uint, struct dinode*);
void rinode(uint inum, struct dinode *ip);
void wsect(uint, void*);
void rsect(uint sec, void *buf);
uint ialloc(ushort type);
void wimap();
void wchkpt(int chkpt_no);
void iappend(uint inum, void *p, int n);

// convert to intel byte order
ushort
xshort(ushort x)
{
  ushort y;
  uchar *a = (uchar*)&y;
  a[0] = x;
  a[1] = x >> 8;
  return y;
}

uint
xint(uint x)
{
  uint y;
  uchar *a = (uchar*)&y;
  a[0] = x;
  a[1] = x >> 8;
  a[2] = x >> 16;
  a[3] = x >> 24;
  return y;
}

int
main(int argc, char *argv[])
{
  int i, cc, fd;
  uint rootino, inum, off;
  struct dirent de;
  char buf[BSIZE];
  struct dinode din;


  static_assert(sizeof(int) == 4, "Integers must be 4 bytes!");

  if(argc < 2){
    fprintf(stderr, "Usage: mkfs fs.img files...\n");
    exit(1);
  }

  assert((BSIZE % sizeof(struct dinode)) == 0);
  assert((BSIZE % sizeof(struct dirent)) == 0);

  fsfd = open(argv[1], O_RDWR|O_CREAT|O_TRUNC, 0666);
  if(fsfd < 0){
    perror(argv[1]);
    exit(1);
  }

  // 1 fs block = 1 disk sector

  sb.magic = FSMAGIC;
  sb.size = xint(FSSIZE);
  sb.nblocks = xint(nblocks);
  sb.nsegments = xint(NSEG);
  sb.ninodes = xint(NINODES);
  sb.checkpoint1 = xint(2);
  sb.checkpoint2 = xint(3);
  sb.segstart = xint(NMETA);

  printf("nmeta %d (boot, super, checkpoint1, checkpoint2) blocks %d total %d\n",
         NMETA, nblocks, FSSIZE);

  freeblock = NMETA + NINODEMAP;     // the first free block that we can allocate

  for(i = 0; i < FSSIZE; i++)
    wsect(i, zeroes);

  bzero(imp, sizeof(imp));

  memset(buf, 0, sizeof(buf));
  memmove(buf, &sb, sizeof(sb));
  wsect(1, buf);

  rootino = ialloc(T_DIR);
  assert(rootino == ROOTINO);

  bzero(&de, sizeof(de));
  de.inum = xshort(rootino);
  strcpy(de.name, ".");
  iappend(rootino, &de, sizeof(de));

  bzero(&de, sizeof(de));
  de.inum = xshort(rootino);
  strcpy(de.name, "..");
  iappend(rootino, &de, sizeof(de));

  for(i = 2; i < argc; i++){
    // get rid of "user/"
    char *shortname;
    if(strncmp(argv[i], "user/", 5) == 0)
      shortname = argv[i] + 5;
    else
      shortname = argv[i];
    
    assert(index(shortname, '/') == 0);

    if((fd = open(argv[i], 0)) < 0){
      perror(argv[i]);
      exit(1);
    }

    // Skip leading _ in name when writing to file system.
    // The binaries are named _rm, _cat, etc. to keep the
    // build operating system from trying to execute them
    // in place of system binaries like rm and cat.
    if(shortname[0] == '_')
      shortname += 1;

    inum = ialloc(T_FILE);

    bzero(&de, sizeof(de));
    de.inum = xshort(inum);
    strncpy(de.name, shortname, DIRSIZ);
    iappend(rootino, &de, sizeof(de));

    while((cc = read(fd, buf, sizeof(buf))) > 0)
      iappend(inum, buf, cc);

    close(fd);
  }

  // fix size of root inode dir
  rinode(rootino, &din);
  off = xint(din.size);
  off = ((off/BSIZE) + 1) * BSIZE;
  din.size = xint(off);
  winode(rootino, &din);

  wimap();
  wchkpt(1);
  wchkpt(2);

  printf("balloc: first %d blocks have been allocated\n", freeblock);

  exit(0);
}

void
winode(uint inum, struct dinode *ip)
{
  char buf[BSIZE];
  uint bn;
  struct dinode *dip;

  bn = IBLOCK(inum, imp);
  rsect(bn, buf);
  dip = (struct dinode*)buf;
  *dip = *ip;
  wsect(bn, buf);
}

void
rinode(uint inum, struct dinode *ip)
{
  char buf[BSIZE];
  uint bn;
  struct dinode *dip;

  bn = IBLOCK(inum, imp);
  rsect(bn, buf);
  dip = (struct dinode*)buf;
  *ip = *dip;
}

void
wsect(uint sec, void *buf)
{
  if(lseek(fsfd, sec * BSIZE, 0) != sec * BSIZE){
    perror("lseek");
    exit(1);
  }
  if(write(fsfd, buf, BSIZE) != BSIZE){
    perror("write");
    exit(1);
  }
}

void
rsect(uint sec, void *buf)
{
  if(lseek(fsfd, sec * BSIZE, 0) != sec * BSIZE){
    perror("lseek");
    exit(1);
  }
  if(read(fsfd, buf, BSIZE) != BSIZE){
    perror("read");
    exit(1);
  }
}

uint
ialloc(ushort type)
{
  uint inum = freeinode++;
  struct dinode din;

  bzero(&din, sizeof(din));
  din.type = xshort(type);
  din.nlink = xshort(1);
  din.size = xint(0);
  imp[inum] = freeblock++;
  winode(inum, &din);
  return inum;
}

void
wimap() {
  char buf[BSIZE];
  int i, j;
  struct dimap *dimp;
  
  for(i=0;i<NINODEMAP;i++) {
    bzero(buf, BSIZE);
    dimp = (struct dimap*)buf;
    for(j=0;j<NENTRY && i*NENTRY + j < NINODES;j++)
      dimp->addr[j] = xint(imp[i*NENTRY + j]);
    wsect(NMETA+i, buf);
  }
}

// chkpt_no : 1 or 2
void wchkpt(int chkpt_no) {
  char buf[BSIZE];
  int i, used_segment;
  struct checkpoint *chkpt;

  bzero(buf, BSIZE);
  if (chkpt_no == 1) {
    chkpt = (struct checkpoint*)buf;

    // write imap location
    for(i=0;i<NINODEMAP;i++)
      chkpt->imap[i] = xint(NMETA+i);
    
    // write segment usage table (bitmap)
    used_segment = (freeblock - 4 + SEGSIZE - 1) / SEGSIZE;
    for(i = 0; i < used_segment; i++)
      chkpt->segtable[i/8] = chkpt->segtable[i/8] | (0x1 << (i%8));
    
    // write timestamp
    chkpt->timestamp = xint(1);
  }
  wsect(1+chkpt_no, buf);
}

#define min(a, b) ((a) < (b) ? (a) : (b))

void
iappend(uint inum, void *xp, int n)
{
  char *p = (char*)xp;
  uint fbn, off, n1;
  struct dinode din;
  char buf[BSIZE];
  uint indirect[NINDIRECT];
  uint x;

  rinode(inum, &din);
  off = xint(din.size);
  // printf("append inum %d at off %d sz %d\n", inum, off, n);
  while(n > 0){
    fbn = off / BSIZE;
    assert(fbn < MAXFILE);
    if(fbn < NDIRECT){
      if(xint(din.addrs[fbn]) == 0){
        din.addrs[fbn] = xint(freeblock++);
      }
      x = xint(din.addrs[fbn]);
    } else {
      if(xint(din.addrs[NDIRECT]) == 0){
        din.addrs[NDIRECT] = xint(freeblock++);
      }
      rsect(xint(din.addrs[NDIRECT]), (char*)indirect);
      if(indirect[fbn - NDIRECT] == 0){
        indirect[fbn - NDIRECT] = xint(freeblock++);
        wsect(xint(din.addrs[NDIRECT]), (char*)indirect);
      }
      x = xint(indirect[fbn-NDIRECT]);
    }
    n1 = min(n, (fbn + 1) * BSIZE - off);
    rsect(x, buf);
    bcopy(p, buf + off - (fbn * BSIZE), n1);
    wsect(x, buf);
    n -= n1;
    off += n1;
    p += n1;
  }
  din.size = xint(off);
  winode(inum, &din);
}
