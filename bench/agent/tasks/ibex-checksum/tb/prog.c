// A message router: a producer fills a queue with messages, a consumer takes them out in order
// of priority and sends each down a link as a frame: a tag byte, then the message's words; after
// each round the receiver parses the new frames back. The checksum of all it got is returned.
#include <stdint.h>

struct msg { uint32_t id, len, data; };

#define QLEN 8
#define FRAME 13
static struct msg queue[QLEN];
static int count;
static uint8_t link[32 * FRAME];
static int sent;

static uint32_t seed = 0x1234;
static uint32_t rnd(void) { seed = seed * 1103515245u + 12345u; return seed >> 7; }

static void put(const struct msg *m) { queue[count++] = *m; }

static void take(struct msg *m) {
  int best = 0;
  for (int i = 1; i < count; i++)
    if ((queue[i].id & 0xf) < (queue[best].id & 0xf)) best = i;
  *m = queue[best];
  queue[best] = queue[--count];
}

static void put32(uint8_t *p, uint32_t w) {
  for (int i = 0; i < 4; i++) p[i] = w >> 8 * i;
}

// Ibex reads a word at any address
static uint32_t get32(const uint8_t *p) { return *(const uint32_t *)p; }

static void send(const struct msg *m) {
  uint8_t *f = &link[sent++ * FRAME];
  f[0] = 0xa0 | (m->id & 0xf);
  put32(f + 1, m->id);
  put32(f + 5, m->len);
  put32(f + 9, m->data);
}

// the receiver: frames sent since the last call, parsed back into messages
static struct msg got[32];
static int received;
static int receive(void) {
  for (; received < sent; received++) {
    const uint8_t *f = &link[received * FRAME];
    if ((f[0] & 0xf0) != 0xa0) return 0;
    got[received].id = get32(f + 1);
    got[received].len = get32(f + 5);
    got[received].data = get32(f + 9);
  }
  return 1;
}

static uint32_t mix(uint32_t c, uint32_t w) { c = (c ^ w) * 0x9e3779b1u; return c ^ c >> 15; }

uint32_t main(void) {
  uint32_t sum = 0;
  for (int round = 0; round < 8; round++) {
    while (count < QLEN) {
      struct msg m = { rnd(), rnd() % 1500, rnd() };
      put(&m);
    }
    for (int k = 0; k < 4; k++) {
      struct msg m;
      take(&m);
      send(&m);
    }
    if (!receive()) return 0;
  }
  for (int i = 0; i < received; i++) {
    sum = mix(sum, got[i].id);
    sum = mix(sum, got[i].len);
    sum = mix(sum, got[i].data);
  }
  return sum;
}
