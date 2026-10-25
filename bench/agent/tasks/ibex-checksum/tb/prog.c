// A message router: a producer fills a queue with messages, a consumer takes them out in
// order of priority, folds each into a running sum and forwards it to one of four outputs.
// The checksum of everything is returned at the end.
#include <stdint.h>

struct msg { uint32_t id, len, data; };

#define QLEN 8
#define OUTS 4
static struct msg queue[QLEN];
static int count;
static struct msg out[OUTS][16];
static int outn[OUTS];

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

static uint32_t mix(uint32_t c, uint32_t w) { c = (c ^ w) * 0x9e3779b1u; return c ^ c >> 15; }

uint32_t main(void) {
  uint32_t sum = 0;
  for (int round = 0; round < 6; round++) {
    while (count < QLEN) {
      struct msg m = { rnd(), rnd() % 1500, rnd() };
      put(&m);
    }
    for (int k = 0; k < 4; k++) {
      struct msg m;
      take(&m);
      int o = m.id % OUTS;
      if (outn[o] < 16) out[o][outn[o]++] = m;
      sum += m.len;
    }
  }
  for (int o = 0; o < OUTS; o++)
    for (int i = 0; i < outn[o]; i++) {
      sum = mix(sum, out[o][i].id);
      sum = mix(sum, out[o][i].len);
      sum = mix(sum, out[o][i].data);
    }
  return sum;
}
