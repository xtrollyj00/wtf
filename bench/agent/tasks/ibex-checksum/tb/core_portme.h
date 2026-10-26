// CoreMark's port for tb.sv and for the host: no timer, printf or heap; seeds from volatiles.
#ifndef CORE_PORTME_H
#define CORE_PORTME_H
#include <stddef.h>
#include <stdint.h>
#define HAS_FLOAT 0
#define HAS_TIME_H 0
#define USE_CLOCK 0
#define HAS_STDIO 0
#define HAS_PRINTF 0
#define COMPILER_VERSION "clang"
#define COMPILER_FLAGS "-O2"
#define MEM_LOCATION "STATIC"
typedef int16_t ee_s16;
typedef uint16_t ee_u16;
typedef int32_t ee_s32;
typedef uint8_t ee_u8;
typedef uint32_t ee_u32;
typedef uintptr_t ee_ptr_int;
typedef size_t ee_size_t;
typedef ee_u32 CORE_TICKS;
#define align_mem(x) (void *)(x)
#define SEED_METHOD SEED_VOLATILE
#define MEM_METHOD MEM_STATIC
#define MULTITHREAD 1
#define MAIN_HAS_NOARGC 1
#define MAIN_HAS_NORETURN 0
typedef struct CORE_PORTABLE_S { ee_u8 portable_id; } core_portable;
extern ee_u32 default_num_contexts;
int ee_printf(const char *fmt, ...);
#endif
