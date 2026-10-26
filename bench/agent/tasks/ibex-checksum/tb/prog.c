// CoreMark's list, matrix and state benchmarks (vendor/eembc_coremark in the Ibex checkout), one
// iteration on the validation seeds, as core_main.c runs them; returns its list, matrix and state
// CRCs as a checksum, crclist << 16 ^ crcmatrix << 8 ^ crcstate.
#include "coremark.h"

volatile ee_s32 seed1_volatile = 0, seed2_volatile = 0, seed3_volatile = 0x66;
volatile ee_s32 seed4_volatile = 1, seed5_volatile = 0;
ee_u32 default_num_contexts = 1;
int ee_printf(const char *fmt, ...) { (void)fmt; return 0; }

ee_s32 get_seed_32(int i);
#define get_seed(x) (ee_s16)get_seed_32(x)

static ee_u8 memblk[TOTAL_DATA_SIZE];

uint32_t main(void) {
  static core_results r;
  r.seed1 = get_seed(1);
  r.seed2 = get_seed(2);
  r.seed3 = get_seed(3);
  r.iterations = get_seed_32(4);
  r.execs = ALL_ALGORITHMS_MASK;
  r.memblock[0] = memblk;
  r.size = TOTAL_DATA_SIZE / NUM_ALGORITHMS;
  for (int i = 0; i < NUM_ALGORITHMS; i++)
    r.memblock[i + 1] = (char *)r.memblock[0] + r.size * i;
  r.list = core_list_init(r.size, r.memblock[1], r.seed1);
  core_init_matrix(r.size, r.memblock[2], (ee_s32)r.seed1 | ((ee_s32)r.seed2 << 16), &r.mat);
  core_init_state(r.size, r.seed1, r.memblock[3]);
  for (ee_u32 i = 0; i < r.iterations; i++) {
    r.crc = crcu16(core_bench_list(&r, 1), r.crc);
    r.crc = crcu16(core_bench_list(&r, -1), r.crc);
    if (i == 0) r.crclist = r.crc;
  }
  return (ee_u32)r.crclist << 16 ^ (ee_u32)r.crcmatrix << 8 ^ r.crcstate;
}
