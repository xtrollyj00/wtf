// prog.c's checksum computed on the host: what the core must return.
#include <stdint.h>
#include <stdio.h>
uint32_t prog_main(void);
int main(void) { printf("%08x\n", prog_main()); return 0; }
