## 16.6 Protocols and decoders on streams

These read streams rather than signals: their input is a stream of another instance, or one of
their own, so an instance of one is decoded after the streams it reads exist. They are not
built-ins with roles: `protocols` lists them, `protocols <name>` says the rest.

- **Decoders** implement one trait, `Decoder { schema, feed(record) -> records }`, made by name
  (`frame`, `spi`, `uart`). `pair`, `join` (several request streams answered by one, in order, as
  AXI-Lite's AW and W by B, or by an ID, as AXI4's B by BID) and `check` work on whole streams
  instead, as functions; built-in protocols use them as steps too.

### frame

Records of a stream grouped into frames, by a last flag, a gap or a select.

```
roles: input (a stream)
params: last=<field> (a frame ends at a record where it is 1), gap=<time> (before a longer gap),
  select=<field> (when it changes), key=<field> (a frame per value of it, side by side),
  bytes=<field|n> (len: a field's 1 bits, or n per record), data=<field> (data: the bytes as hex text)
fields: beats, the select field, the key field, len (with bytes), data (with data), complete (0: the input ended first)
example: decode f frame input=rx.beat last=tlast bytes=tkeep data=tdata
```

### pair

Each response with its request, by order or by an id field, with the delay.

```
roles: request, response (streams)
params: id=<field> (the same in both), or req_id=<field> resp_id=<field>; none: by order
fields: delay (from the request to its response), req_<each request field>, resp_<each response field>
also <name>.unmatched (violations): kind = no request | no response
example: decode p pair request=cmd response=rsp id=tag
```

### check

A violation record per clock cycle where a rule holds.

```
roles: none; needs clock=
params: <rule>=<expression>, any number; with rose() fell() stable() past()
fields: rule (the name of the rule that held); empty: no violations
example: decode stall check clock=clk waiting=valid && !ready dropped=fell(valid) && !past(ready)
```

### join

Several request streams answered by one response stream, in order (AXI-Lite's AW and W answered
by B) or by an ID field (AXI4's B by its BID). A request is `no request` when the response comes
with nothing to pair it with, and `no response` when the next response leaves it unanswered past
`timeout`. It is a function, not a protocol of its own: `axilite` and `axi4` make `wr` and `rd`
with it.
