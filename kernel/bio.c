// Buffer cache.
//
// The buffer cache is a linked list of buf structures holding
// cached copies of disk block contents.  Caching disk blocks
// in memory reduces the number of disk reads and also provides
// a synchronization point for disk blocks used by multiple processes.
//
// Interface:
// * To get a buffer for a particular disk block, call bread.
// * After changing buffer data, call bwrite to write it to disk.
// * When done with the buffer, call brelse.
// * Do not use the buffer after calling brelse.
// * Only one process at a time can use a buffer,
//     so do not keep them longer than necessary.



#include "types.h"
#include "param.h"
#include "riscv.h"
#include "defs.h"
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <pthread.h>
#include "fs.h"
#include "buf.h"
#include "virtio_disk.h"

#define NBUF 8

struct buf {
  uint32_t dev;
  uint32_t blockno;
  bool valid;
  int refcnt;
  pthread_mutex_t lock;
  uint8_t data[BLOCK_SIZE];
  struct buf* next;
  struct buf* prev;
};

struct buf_cache {
  pthread_mutex_t lock;
  struct buf buf[NBUF];
  struct buf head;
};

static struct buf_cache bcache;

void buf_cache_init(void) {
  pthread_mutex_init(&bcache.lock, NULL);

  // Create linked list of buffers
  bcache.head.prev = &bcache.head;
  bcache.head.next = &bcache.head;
  for (size_t i = 0; i < NBUF; i++) {
    struct buf* b = &bcache.buf[i];
    pthread_mutex_init(&b->lock, NULL);
    b->next = bcache.head.next;
    b->prev = &bcache.head;
    bcache.head.next->prev = b;
    bcache.head.next = b;
  }
}

// Look through buffer cache for block on device dev.
// If not found, allocate a buffer.
// In either case, return locked buffer.
static struct buf* bget(uint32_t dev, uint32_t blockno) {
  pthread_mutex_lock(&bcache.lock);

  // Check if the block is already cached.
  struct buf* b;
  for (b = bcache.head.next; b != &bcache.head; b = b->next) {
    if (b->dev == dev && b->blockno == blockno) {
      b->refcnt++;
      pthread_mutex_unlock(&bcache.lock);
      pthread_mutex_lock(&b->lock);
      return b;
    }
  }

  // Block is not cached.
  // Recycle the least recently used (LRU) unused buffer.
  for (b = bcache.head.prev; b != &bcache.head; b = b->prev) {
    if (b->refcnt == 0) {
      b->dev = dev;
      b->blockno = blockno;
      b->valid = false;
      b->refcnt = 1;
      pthread_mutex_unlock(&bcache.lock);
      pthread_mutex_lock(&b->lock);
      return b;
    }
  }

  // All buffers are in use.
  pthread_mutex_unlock(&bcache.lock);
  return NULL;
}



// Return a locked buf with the contents of the indicated block.
// If no buffer is available or if the read from the disk fails, return NULL.
struct buf* bread(uint32_t dev, uint32_t blockno) {
  struct buf* b = bget(dev, blockno);
  if (b == NULL) {
    return NULL;
  }

  if (!b->valid) {
    if (!virtio_disk_rw(b, 0)) {
      pthread_mutex_unlock(&b->lock);
      return NULL;
    }
    b->valid = true;
  }

  return b;
}

// Write b's contents to disk.  Must be locked.
// Return true if successful, false otherwise.
bool bwrite(struct buf* b) {
  if (!pthread_mutex_trylock(&b->lock)) {
    return false;
  }

  bool success = virtio_disk_rw(b, 1);
  pthread_mutex_unlock(&b->lock);
  return success;
}

// Release a locked buffer.
// Move to the head of the most-recently-used list.
void brelse(struct buf* b) {
  pthread_mutex_lock(&bcache.lock);
  b->refcnt--;
  if (b->refcnt == 0) {
    // Move to the head of the most-recently-used list.
    b->next->prev = b->prev;
    b->prev->next = b->next;
    b->next = bcache.head.next;
    b->prev = &bcache.head;
    bcache.head.next->prev = b;
    bcache.head.next = b;
  }
  pthread_mutex_unlock(&bcache.lock);
  pthread_mutex_unlock(&b->lock);
}

