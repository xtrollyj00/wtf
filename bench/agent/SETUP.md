# Setting up the agent benchmark

`wtf-bench agent` runs `claude -p` on the tasks in `tasks/`. The trace tasks (`*.toml`) need only
wtf and the `claude` CLI. The fix tasks simulate RTL: they need a simulator, the libraries it
links, and a Python environment for their cocotb testbenches. This is what they need, in the
order to set it up, and what each failure looks like when a piece is missing.

| Tasks | Needs |
|---|---|
| `eth-parser-headers`, `eth-parser-stall`, `head-trimmer-boundary`, `axil2mi-stale-addr` | nvc; nvc's Xilinx libraries (UNISIM, XPM) compiled by that nvc; Python 3.11+ with a shared libpython; cocotb 2 and the packages below |
| `ibex-checksum` | Verilator 5 with lz4 (its FST writer includes `lz4.h` and links `-llz4`) |

Without Verilator, leave `ibex-checksum` out of the task list; the others don't need it.

## 1. nvc and its Xilinx libraries

The ndk-fpga designs use Xilinx primitives, which nvc finds in `~/.nvc/lib` (`unisim`, `xpm`,
...). They must be compiled by the nvc that runs the tasks: nvc's library format changes between
versions, and a library from an older nvc stops the build with

```
** Fatal: ~/.nvc/lib/unisim.08/UNISIM.VCOMPONENTS: serialised format digest is ... expected ...
This design unit uses a library format from an earlier version of nvc and should be reanalysed.
```

Compile them again after every nvc upgrade (`nvc --install vivado`, from Vivado's sources). UNISIM
and XPM are both used; reinstalling only one moves the error to the other.

## 2. A Python environment for cocotb

The testbenches need cocotb 2 (the Makefiles call `cocotb-config --pygpi-entry-point`),
`cocotb-bus`, `cocotbext-axi`, and ndk-fpga's own `ofm` and `cocotbext` packages (`cocotbext.ofm`),
which need Python 3.11 or newer.

cocotb loads libpython into the simulator, so the Python must have a shared libpython: check that
`cocotb-config --libpython` prints a path. A distribution's `python3.12` package without its
library prints nothing, and the simulation stops with `undefined symbol: PyExc_SystemExit`. uv's
own Pythons have one:

```sh
uv venv --python 3.12 --python-preference only-managed ~/.venvs/wtf-bench
export VIRTUAL_ENV=~/.venvs/wtf-bench
uv pip install cocotb cocotb-bus cocotbext-axi tabulate numpy pandas matplotlib seaborn Pillow colorama pyyaml
```

ndk-fpga's packages come from its checkout at the tasks' commit, which `agent check` or `agent run`
fetches into `target/upstream/<commit>/` the first time (step 3 does it; or clone
`https://github.com/CESNET/ndk-fpga.git` at the `commit` in a task's `task.toml`):

```sh
NDK=target/upstream/aac31e7771740a5ea6cc3ee2623a1318dd579fd0
NDK_FPGA_OFM_URL=file://$PWD/$NDK/python/ofm uv pip install --no-deps $NDK/python/ofm $NDK/python/cocotbext
```

`--no-deps` because `ofm` depends on `nfb`, a C extension that needs CESNET's libnfb headers to
build and that the tests don't use; its other dependencies are in the first install.
`NDK_FPGA_OFM_URL` is how `cocotbext`'s `pyproject.toml` names its `ofm`; without it uv stops at
`relative path without a working directory: ${NDK_FPGA_OFM_URL}`.

The bench and the agents' `make sim` find cocotb on `PATH`:

```sh
export PATH=~/.venvs/wtf-bench/bin:$PATH
```

## 3. Prove the tasks before paying for runs

```sh
cargo build --release
cargo run -p wtf-bench --release -- agent check eth-parser-headers eth-parser-stall head-trimmer-boundary axil2mi-stale-addr
```

`check` runs no agent and costs nothing: each task's bug must fail its `sim` and `hidden` tests
and the upstream files must pass both, 16 lines of `ok`. A `WRONG` line names a directory in
`/tmp` with the logs (`sim.log`, `hidden.log`); to rerun its `make` by hand, give it the checkout,
`make sim UPSTREAM=$PWD/target/upstream/<commit>`.

`head-trimmer-boundary`'s hidden test is ndk-fpga's own regression, 4 tests with jumbo packets: it
took 17 minutes on a slow machine (the others take seconds to minutes). It runs again to grade
each run of that task.

## 4. Run

```sh
cargo run -p wtf-bench --release -- agent run --model claude-haiku-5-5 --budget 0.5 [--runs N] [<task>...]
cargo run -p wtf-bench --release -- agent report
```

Runs use the subscription of the logged-in `claude`: start small (one run per task), and keep
`--timeout` at its 600 s, which is part of the measure.

## When something is missing

| Symptom | Cause | Fix |
|---|---|---|
| `cocotb-config: not found` in a task's log | the environment is not on `PATH` | step 2, the `export PATH` |
| `serialised format digest is ... should be reanalysed` | a Xilinx library compiled by another nvc | step 1, for every library named |
| `No module named 'cocotb'` / `'cocotbext.axi'` / `'cocotbext.ofm'` | a package missing | step 2 |
| `ofm==0.1.0 depends on Python>=3.11` | the environment's Python is too old | step 2, `--python 3.12` |
| `nfb ... Python.h` while installing `ofm` | `nfb` needs libnfb to build | install `ofm` with `--no-deps` |
| `relative path without a working directory: ${NDK_FPGA_OFM_URL}` | `cocotbext` names `ofm` by that variable | set `NDK_FPGA_OFM_URL=file://...` |
| `undefined symbol: PyExc_SystemExit`; `cocotb-config --libpython` empty | the Python has no shared libpython | a uv-managed Python (`--python-preference only-managed`) |
| `UPSTREAM: the ndk-fpga checkout` | `make` run by hand | `make ... UPSTREAM=$PWD/target/upstream/<commit>` |
| `lz4.h: No such file` (ibex-checksum) | Verilator's FST writer needs lz4 | the lz4 development package, or leave the task out |
