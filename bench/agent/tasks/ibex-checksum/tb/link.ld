MEMORY { ram : ORIGIN = 0, LENGTH = 64K }
SECTIONS {
  .vectors 0 : { KEEP(*(.vectors)) } > ram
  .text 0x80 : { *(.text.start) *(.text*) } > ram
  .data : { *(.rodata*) *(.sdata*) *(.data*) *(.sbss*) *(.bss*) } > ram
  _stack = ORIGIN(ram) + LENGTH(ram);
}
