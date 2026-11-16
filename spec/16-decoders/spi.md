## 16.4 SPI (spi)

SPI is a `Protocol`, not a `Decoder`: it takes no `clock`. A `Protocol` that is not clocked is fed
each change of its roles' signals instead, in time order, so it keeps the edges and bit times
itself. Its settings are params of the instance: `mode`, `bits`, `order`, `cs_active` (`low`, the
default, or `high`). An SPI bit lasts to the next sampling edge, the last of a transfer to sclk's
next edge. So `spi` is no entry of the decoder registry, which has `frame` and `uart`.

```
decode flash spi sclk=spi_clk cs=spi_cs_n mosi=spi_mosi miso=spi_miso mode=0
```

| role | required | it is |
| --- | --- | --- |
| `sclk` | yes | the clock; its sampling edges are where a bit is read |
| `cs` | yes | chip select, active low (`cs_active=high`: active high) |
| `mosi` | yes | the data out to the device |
| `miso` | no | the data in from it (no `miso` field without it) |

Params: `mode` (`0` - `3`: rising samples in 0 and 3, falling in 1 and 2), `bits` (`8`, 1 - 64),
`order` (`msb`, or `lsb`), `cs_active` (`low`, or `high`).

Streams:

| stream | it is |
| --- | --- |
| `<name>.bit` | a bit of mosi (and miso) at each sampling edge of sclk while cs is low (mode 0 and 3: rising, 1 and 2: falling) |
| `<name>.word` | `bits` bits (8), msb or lsb first by `order`; `complete` 0 when cs rose first |
| `<name>.transfer` | cs low to high, its words as hex text; `complete` 0 when the trace ended first |
