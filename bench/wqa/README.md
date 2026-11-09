# WaveformQA-style questions

Questions about a waveform with one right answer, asked of an agent that reads the trace with
wtf's tools or with the shell. The 24 kinds of question are those of WaveformQA (Liu, Cummings,
Vadlamudi, "WaveformQA: Benchmarking LLM Temporal Reasoning on Digital Waveforms",
arXiv:2607.20638, Table IV), written again from the paper: these are not its questions, which
are not published, so the numbers don't compare with its numbers.

- `vcd.py` is the referee: a small VCD reader that computes every answer, so wtf never grades
  itself.
- `gen.py` draws the questions. A draw that a reading of the question could change (a value
  changing at the instant asked, a change on a window's end, a tie in an order) is thrown away;
  a sampled condition says so ("its value just before the edge").
- `run.py` asks `claude -p` each question in one arm, `wtf` (wtf's MCP tools only) or `shell`
  (Bash and Read), and scores the `ANSWER:` line: values as numbers, periods within 1%, lists in
  order.
- `mcpcheck.py` checks that wtf resolves every signal the questions name.

## The traces

The paper's 13 VCDs, from five RISC-V cores, are on Hugging Face (`dcummingsTT/waveformqa`,
CC-BY-4.0, Copyright 2026 Tenstorrent USA, Inc.; the cores' own licenses and attributions are in
the archive's `LICENSE`). About 364 MB, 3.3 GB unpacked:

```sh
mkdir -p target/wqa/vcds && cd target/wqa/vcds
curl -LO https://huggingface.co/datasets/dcummingsTT/waveformqa/resolve/main/waveformqa_vcds.zip
unzip waveformqa_vcds.zip
```

`darkriscv_coremark.vcd` ends mid-line; the referee skips the incomplete record. On a trace
over 100 MB the referee reads the clocks, state-like signals and a seeded sample of 600 others,
to stay in memory.

## Two sets

The dev set was read while improving wtf; the held-out set is for reporting and is not read.

```sh
W=target/wqa
# dev: 5 a kind on the eight traces of the dev set (seed 7)
python3 bench/wqa/gen.py $W/vcds $W/dev.jsonl 5
# held out: 5 a kind on five other traces (seed 1109)
python3 bench/wqa/gen.py $W/vcds $W/heldout.jsonl 5 1109 \
    serv_hello_world,darkriscv_dhrystone,darkriscv_mandelbrot,biriscv_custom,ibex_custom
```

## Run

Each run costs money (Haiku: about $0.003 a question). Keep the arms on the same questions and
model:

```sh
cargo build --release
python3 bench/wqa/run.py wtf   $W/heldout.jsonl $W/vcds $W/heldout-wtf.jsonl   --jobs 3
python3 bench/wqa/run.py shell $W/heldout.jsonl $W/vcds $W/heldout-shell.jsonl --jobs 3
```

Each line of the output has the answer, whether it is right, cost, turns, tool calls and wtf's
errors; the agents' logs are beside it in `<out>.logs/`. Results are in `bench/RESULTS.md`.
