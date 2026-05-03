SECTIONS
{
  .rp2350_image_def : ALIGN(4)
  {
    KEEP(*(.rp2350_image_def));
  } > FLASH
} INSERT AFTER .vector_table;
