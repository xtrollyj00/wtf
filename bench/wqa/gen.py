"""WaveformQA-style questions over the paper's traces: its 24 subcategories (Table IV), each
worded so it has one answer, with the answer computed by vcd.py.

A sample that a reading of the question could change (a value changing at the instant asked, a
change on a window's end, a tie in an order) is thrown away and another drawn."""

import json
import random
import sys

import vcd

TRACES = ["picorv32_custom", "serv_custom", "ibex_custom", "biriscv_testbench",
          "darkriscv_coremark", "ibex_dhrystone", "biriscv_tcm_soc", "picorv32_dhrystone"]
BIG = 100_000_000  # past this, the referee reads the clocks and a sample of the signals: memory


def load(path, name):
    import os
    if os.path.getsize(path) <= BIG:
        return vcd.read(path)
    sigs = vcd.header(path)
    r = random.Random(f"sample-{name}")
    leaf = lambda p: p.rsplit(".", 1)[1].lower()
    keep = {p for p, w in sigs if w == 1 and "clk" in leaf(p)}
    keep |= {p for p, w in sigs if 2 <= w <= 8 and any(k in leaf(p) for k in ("state", "fsm", "_cs"))}
    keep |= set(r.sample([p for p, _ in sigs], min(600, len(sigs))))
    return vcd.read(path, lambda p, w: p in keep)


def nth(k):
    return f"{k}{'th' if 10 <= k % 100 <= 20 else {1: 'st', 2: 'nd', 3: 'rd'}.get(k % 10, 'th')}"


def hx(v):
    return f"0x{v:x}"


class Trace:
    def __init__(self, name, sigs, unit, end):
        self.name, self.sigs, self.end = name, sigs, end
        # a timescale of 10ps counts ticks of 10ps: questions say times in ps
        digits = unit[: len(unit) - len(unit.lstrip("0123456789"))]
        self.mult, self.unit = int(digits or 1), unit[len(digits):].strip()
        live = [s for s in sigs.values() if len(s.changes) >= 6]
        by_edges = sorted((s for s in live if s.width == 1), key=lambda s: (-len(s.edges()), len(s.path)))
        self.clk = by_edges[0]
        self.rise = self.clk.edges()
        self.period = (self.rise[-1] - self.rise[0]) / (len(self.rise) - 1)
        busy = len(self.rise) / 2
        self.bits = [s for s in live if s.width == 1 and len(s.changes) < busy and s.path.rsplit(".", 1)[1] != self.clk.path.rsplit(".", 1)[1]]
        self.data = [s for s in live if s.width > 1]
        self.fsm = []
        for s in live:
            if 2 <= s.width <= 8:
                vals = {v for _, v in s.changes if v is not None}
                if 3 <= len(vals) <= 12:
                    self.fsm.append(s)
        named = [s for s in self.fsm if any(w in s.path.rsplit(".", 1)[1].lower() for w in ("state", "fsm", "_cs"))]
        self.fsm = named or self.fsm

    def t(self, rng, lo=0.2, hi=0.8):
        return int(self.end * lo + rng.random() * self.end * (hi - lo))

    def span(self, rng, cycles=(20, 200)):
        t1 = self.t(rng)
        return t1, t1 + int(self.period * rng.randint(*cycles))

    def u(self, t):
        return f"{t * self.mult}{self.unit}"


def clean(s, t1, t2):
    """s has a known value over the whole of [t1, t2]."""
    if s.at(t1) is None:
        return False
    return all(v is not None for t, v in s.changes if t1 <= t <= t2)


def after(times, t, k):
    """The k-th time in `times` after t (1-based), or None."""
    xs = [x for x in times if x > t]
    return xs[k - 1] if len(xs) >= k else None


def at_every(sigs, t1, t2):
    """Every time in [t1, t2] at which one of sigs changes, in order."""
    return sorted({t for s in sigs for t, _ in s.changes if t1 <= t <= t2})


SAMPLE = "(its value just before the edge, as a flip-flop samples it)"

# Each generator gets the trace and a random generator, and returns (question, answer, kind) or None.
# kind: "value" (a number, hex or decimal), "number" (within 1%), "word", "list", "names" (signal paths).


def lookup_value_at_edge(tr, r):
    s = r.choice(tr.data + tr.bits)
    T, k = tr.t(r), r.randint(3, 20)
    e = after(tr.rise, T, k)
    if e is None or not clean(s, T, e):
        return None
    return f"What value of {s.path} does the {nth(k)} rising edge of {tr.clk.path} after t={tr.u(T)} sample {SAMPLE}?", hx(s.before(e)), "value"


def lookup_value_at_time(tr, r):
    s = r.choice(tr.data)
    T = tr.t(r)
    if not clean(s, T, T) or s.changes_at(T):
        return None
    return f"What is the value of {s.path} at t={tr.u(T)}?", hx(s.at(T)), "value"


def lookup_value_when_changes(tr, r):
    q, trig = r.choice(tr.data), r.choice(tr.data + tr.bits)
    T, k = tr.t(r), r.randint(1, 4)
    ch = [(t, v) for t, v in trig.changes if t > T]
    if len(ch) < 2:
        return None
    # the k-th time trig goes from a to b, for the a -> b of its first change after T
    i = next((j for j, (t, _) in enumerate(trig.changes) if t > T), None)
    a, b = trig.changes[i - 1][1], trig.changes[i][1]
    hits = [trig.changes[j][0] for j in range(i, len(trig.changes)) if trig.changes[j - 1][1] == a and trig.changes[j][1] == b]
    if a is None or b is None or len(hits) < k:
        return None
    t = hits[k - 1]
    if not clean(q, T, t) or q.changes_at(t):
        return None
    return f"What is the value of {q.path} when {trig.path} transitions from {hx(a)} to {hx(b)} for the {nth(k)} time after t={tr.u(T)}?", hx(q.at(t)), "value"


def counting_transitions(tr, r):
    s = r.choice(tr.data + tr.bits)
    t1, t2 = tr.span(r)
    if not clean(s, t1, t2) or s.changes_at(t1) or s.changes_at(t2):
        return None
    n = sum(1 for t, _ in s.changes if t1 <= t <= t2)
    if n < 2:
        return None
    return f"How many times does {s.path} change value between t={tr.u(t1)} and t={tr.u(t2)} (inclusive)?", str(n), "value"


def edges_while(s, gate, level, t1, t2):
    """Edges of s in [t1, t2] at which gate, sampled just before the edge, is at level."""
    return sum(1 for e in s.edges() if t1 <= e <= t2 and gate.before(e) == level)


def counting_rising_gated(tr, r):
    s, gate = r.choice(tr.bits), r.choice(tr.bits)
    t1, t2 = tr.span(r, (50, 400))
    if s is gate or not clean(s, t1, t2) or not clean(gate, t1, t2) or s.changes_at(t1) or s.changes_at(t2):
        return None
    n = edges_while(s, gate, 0, t1, t2)
    total = sum(1 for e in s.edges() if t1 <= e <= t2)
    if n is None or n == 0 or n == total:
        return None
    return f"How many rising edges does {s.path} have while {gate.path} is low (its value just before each edge), between t={tr.u(t1)} and t={tr.u(t2)} (inclusive)?", str(n), "value"


def counting_conditional(tr, r):
    cond = r.choice(tr.bits)
    t1, t2 = tr.span(r)
    if not clean(cond, t1 - int(tr.period), t2) or tr.clk.changes_at(t1) or tr.clk.changes_at(t2):
        return None
    n = edges_while(tr.clk, cond, 1, t1, t2)
    total = sum(1 for e in tr.rise if t1 <= e <= t2)
    if n is None or n == 0 or n == total:
        return None
    return f"How many rising edges does {tr.clk.path} have while {cond.path} is high (its value just before each edge), between t={tr.u(t1)} and t={tr.u(t2)} (inclusive)?", str(n), "value"


def arithmetic_period(tr, r):
    t1, t2 = tr.span(r)
    es = [e for e in tr.rise if t1 <= e <= t2]
    if len(es) < 3:
        return None
    p = (es[-1] - es[0]) / (len(es) - 1) * tr.mult
    return f"What is the average period of {tr.clk.path} between t={tr.u(t1)} and t={tr.u(t2)}, in {tr.unit}?", f"{p:g}", "number"


SECONDS = {"s": 1, "ms": 1e-3, "us": 1e-6, "ns": 1e-9, "ps": 1e-12, "fs": 1e-15}


def arithmetic_frequency(tr, r):
    t1, t2 = tr.span(r)
    es = [e for e in tr.rise if t1 <= e <= t2]
    if len(es) < 3:
        return None
    scale = tr.mult * SECONDS[tr.unit]
    hz = 1 / ((es[-1] - es[0]) / (len(es) - 1) * scale)
    fu, div = ("MHz", 1e6) if hz >= 1e6 else ("Hz", 1)
    return f"What is the frequency of {tr.clk.path} between t={tr.u(t1)} and t={tr.u(t2)}, in {fu}?", f"{hz / div:g}", "number"


def arithmetic_time_difference(tr, r):
    s1, s2 = r.choice(tr.bits), r.choice(tr.bits)
    T, o1, o2 = tr.t(r), r.randint(1, 3), r.randint(1, 3)
    a, b = after(s1.edges(True), T, o1), after(s2.edges(False), T, o2)
    if a is None or b is None or b <= a or (b - a) > 2000 * tr.period:
        return None
    return (f"What is the time from the {nth(o1)} rising edge of {s1.path} to the {nth(o2)} falling edge of {s2.path}, both counted after t={tr.u(T)}, in {tr.unit}?", str((b - a) * tr.mult), "number")


def first_change(s, T):
    return next((t for t, _ in s.changes if t > T), None)


def temporal_before_after(tr, r):
    s1, s2 = r.sample(tr.data + tr.bits, 2)
    T = tr.t(r)
    a, b = first_change(s1, T), first_change(s2, T)
    if a is None or b is None:
        return None
    word = "before" if a < b else "after" if a > b else "same"
    return (f"After t={tr.u(T)}, does {s1.path} first change before, after, or at the same time as {s2.path} first changes? Answer before, after, or same.", word, "word")


def temporal_ordering(tr, r):
    ss = r.sample(tr.data + tr.bits, r.randint(3, 4))
    T = tr.t(r)
    fs = [first_change(s, T) for s in ss]
    if None in fs or len(set(fs)) < len(fs):
        return None
    order = [s.path for _, s in sorted(zip(fs, ss), key=lambda x: x[0])]
    shown = [s.path for s in ss]
    return (f"List these signals in the order they first change after t={tr.u(T)}: {', '.join(shown)}. Give a comma-separated list.", ", ".join(order), "names")


def temporal_last_before(tr, r):
    d, ref = r.choice(tr.data), r.choice(tr.bits)
    T, k = tr.t(r), r.randint(1, 7)
    e = after(ref.edges(), T, k)
    if e is None or not clean(d, T, e):
        return None
    return (f"What is the LAST value of {d.path} before the {nth(k)} rising edge of {ref.path} after t={tr.u(T)}? Give the value immediately preceding that rising edge.", hx(d.before(e)), "value")


def multistep_cycles_after(tr, r):
    d, trig = r.choice(tr.data), r.choice(tr.bits)
    T, n = tr.t(r), r.randint(2, 12)
    t0 = after(trig.edges(), T, 1)
    if t0 is None:
        return None
    e = after(tr.rise, t0, n)
    if e is None or not clean(d, t0, e):
        return None
    return (f"{trig.path} first rises after t={tr.u(T)}. What value of {d.path} does the {nth(n)} rising edge of {tr.clk.path} strictly after that rise sample {SAMPLE}?", hx(d.before(e)), "value")


def multistep_nth_event(tr, r):
    q, ev = r.choice(tr.data), r.choice(tr.bits)
    T, k = tr.t(r), r.randint(2, 18)
    e = after(ev.edges(False), T, k)
    if e is None or not clean(q, T, e) or q.changes_at(e):
        return None
    return f"What is the value of {q.path} immediately after the {nth(k)} falling edge of {ev.path} after t={tr.u(T)}?", hx(q.at(e)), "value"


def multistep_conditional_sequence(tr, r):
    c, a, b = r.choice(tr.data), *r.sample(tr.bits, 2)
    T, k = tr.t(r), r.randint(1, 4)
    tb = after(b.edges(False), T, k)
    ta = after(a.edges(True), tb, 1) if tb is not None else None
    if ta is None or not clean(c, T, ta) or c.changes_at(ta) or ta - tb > 2000 * tr.period:
        return None
    return (f"What is the value of {c.path} when {a.path} goes high for the first time after the {nth(k)} time {b.path} goes low after t={tr.u(T)}?", hx(c.at(ta)), "value")


def becomes_true(conds, t1, t2):
    """The times in (t1, t2] at which every (signal, value) holds and did not just before."""
    out = []
    for t in at_every([s for s, _ in conds], t1 + 1, t2):
        if all(s.at(t) == v for s, v in conds) and not all(s.before(t) == v for s, v in conds):
            out.append(t)
    return out


def correlation_dual(tr, r):
    q = r.choice(tr.data)
    c1, c2 = r.sample(tr.bits, 2)
    T, k = tr.t(r), r.randint(1, 4)
    v1, v2 = r.randint(0, 1), r.randint(0, 1)
    end = T + int(2000 * tr.period)
    hits = becomes_true([(c1, v1), (c2, v2)], T, end)
    if len(hits) < k or not clean(q, T, hits[k - 1]) or q.changes_at(hits[k - 1]):
        return None
    return (f"What is the value of {q.path} at the {nth(k)} time after t={tr.u(T)} that {c1.path}={v1} AND {c2.path}={v2} becomes true?", hx(q.at(hits[k - 1])), "value")


def correlation_triple(tr, r):
    q = r.choice(tr.data)
    cs = r.sample(tr.bits, 3)
    vs = [r.randint(0, 1) for _ in cs]
    t1, t2 = tr.span(r, (100, 1000))
    k = r.randint(1, 3)
    hits = becomes_true(list(zip(cs, vs)), t1, t2)
    if len(hits) < k or not clean(q, t1, hits[k - 1]) or q.changes_at(hits[k - 1]):
        return None
    cond = " AND ".join(f"{c.path}={v}" for c, v in zip(cs, vs))
    return (f"Between t={tr.u(t1)} and t={tr.u(t2)}, what is the value of {q.path} at the {nth(k)} time {cond} becomes true?", hx(q.at(hits[k - 1])), "value")


def correlation_changes_last(tr, r):
    trig = r.choice(tr.bits)
    opts = r.sample([s for s in tr.data + tr.bits if s is not trig], 3)
    t0 = after(trig.edges(), tr.t(r), 1)
    if t0 is None:
        return None
    fs = [first_change(s, t0) for s in opts]
    if None in fs or len(set(fs)) < len(fs) or max(fs) - t0 > 2000 * tr.period:
        return None
    last = opts[fs.index(max(fs))].path
    return (f"After {trig.path} goes high at t={tr.u(t0)}, which of these signals is the last to change (each one's first change after that time): {', '.join(s.path for s in opts)}?", last, "names")


def fsm_next_state(tr, r):
    s = r.choice(tr.fsm)
    T, k = tr.t(r), r.randint(1, 4)
    i = next((j for j, (t, _) in enumerate(s.changes) if t > T), None)
    if i is None or i == 0 or s.changes[i - 1][1] is None:
        return None
    x = s.changes[i - 1][1]
    leaves = [j for j in range(i, len(s.changes)) if s.changes[j - 1][1] == x]
    if len(leaves) < k or s.changes[leaves[k - 1]][1] is None:
        return None
    return (f"When {s.path} leaves state {hx(x)} for the {nth(k)} time after t={tr.u(T)}, what state does it go to?", hx(s.changes[leaves[k - 1]][1]), "value")


def fsm_state_sequence(tr, r):
    s = r.choice(tr.fsm)
    t1, t2 = tr.span(r, (10, 60))
    if not clean(s, t1, t2) or s.changes_at(t1) or s.changes_at(t2):
        return None
    seq = [s.at(t1)] + [v for t, v in s.changes if t1 < t <= t2]
    if not 3 <= len(seq) <= 15:
        return None
    return (f"List the sequence of values {s.path} takes between t={tr.u(t1)} and t={tr.u(t2)}, starting with its value at t={tr.u(t1)}. Give a comma-separated list.", ", ".join(hx(v) for v in seq), "list")


def fsm_return_count(tr, r):
    s = r.choice(tr.fsm)
    t1, t2 = tr.span(r, (100, 2000))
    if not clean(s, t1, t2) or s.changes_at(t1) or s.changes_at(t2):
        return None
    pairs = [(s.changes[j - 1][1], s.changes[j][1]) for j in range(1, len(s.changes)) if t1 < s.changes[j][0] <= t2]
    if not pairs:
        return None
    a, b = r.choice(pairs)
    return (f"How many times does {s.path} transition directly from {hx(a)} to {hx(b)} between t={tr.u(t1)} and t={tr.u(t2)}?", str(pairs.count((a, b))), "value")


def misdirection_nth_occurrence(tr, r):
    q, trig = r.choice(tr.data), r.choice(tr.bits)
    T, k = tr.t(r), r.randint(2, 6)
    es = [e for e in trig.edges() if e > T][:6]
    if len(es) < k or not clean(q, T, es[k - 1]) or q.changes_at(es[k - 1]):
        return None
    shown = ", ".join(tr.u(e) for e in es[:3])
    return (f"{trig.path} has multiple rising edges after t={tr.u(T)}: at {shown}, and more. What is the value of {q.path} immediately after the {nth(k)} of these rising edges?", hx(q.at(es[k - 1])), "value")


def misdirection_after_event(tr, r):
    q, trig, ref = r.choice(tr.data), *r.sample(tr.bits, 2)
    T, k = tr.t(r), r.randint(2, 5)
    tf = after(ref.edges(False), T, k)
    tt = after(trig.edges(True), tf, 1) if tf is not None else None
    if tt is None or not clean(q, T, tt) or q.changes_at(tt) or tt - tf > 2000 * tr.period:
        return None
    return (f"What is the value of {q.path} when {trig.path} goes high for the first time AFTER the {nth(k)} time {ref.path} goes low after t={tr.u(T)}?", hx(q.at(tt)), "value")


def misdirection_ignore_glitch(tr, r):
    s = r.choice(tr.bits)
    # a pulse: two changes close together, with the signal still around them
    i = r.randrange(1, max(2, len(s.changes) - 2))
    if i + 1 >= len(s.changes):
        return None
    (ta, va), (tb, vb) = s.changes[i], s.changes[i + 1]
    before = s.changes[i - 1]
    if None in (va, vb, before[1]) or tb - ta > 2 * tr.period:
        return None
    nxt = s.changes[i + 2][0] if i + 2 < len(s.changes) else tr.end
    pad = int(10 * tr.period)
    t1, t2 = ta - pad, tb + pad
    if before[0] >= t1 or nxt <= t2:
        return None
    return (f"{s.path} has a short pulse between t={tr.u(ta)} and t={tr.u(tb)}. Ignoring such transients, what is the stable value of {s.path} during t={tr.u(t1)} to t={tr.u(t2)}?", hx(vb), "value")


SUBCATEGORIES = [
    ("lookup", "value_at_edge", lookup_value_at_edge),
    ("lookup", "value_at_time", lookup_value_at_time),
    ("lookup", "value_when_changes", lookup_value_when_changes),
    ("counting", "transition_count", counting_transitions),
    ("counting", "rising_edge_gated", counting_rising_gated),
    ("counting", "conditional_count", counting_conditional),
    ("arithmetic", "period", arithmetic_period),
    ("arithmetic", "frequency", arithmetic_frequency),
    ("arithmetic", "time_difference", arithmetic_time_difference),
    ("temporal", "before_after", temporal_before_after),
    ("temporal", "ordering", temporal_ordering),
    ("temporal", "last_before", temporal_last_before),
    ("multistep", "cycles_after_trigger", multistep_cycles_after),
    ("multistep", "nth_event_value", multistep_nth_event),
    ("multistep", "conditional_sequence", multistep_conditional_sequence),
    ("correlation", "dual_condition", correlation_dual),
    ("correlation", "triple_condition", correlation_triple),
    ("correlation", "which_changes_last", correlation_changes_last),
    ("fsm", "next_state", fsm_next_state),
    ("fsm", "state_sequence", fsm_state_sequence),
    ("fsm", "return_count", fsm_return_count),
    ("misdirection", "nth_occurrence", misdirection_nth_occurrence),
    ("misdirection", "after_specific_event", misdirection_after_event),
    ("misdirection", "ignore_glitch", misdirection_ignore_glitch),
]


def main():
    """gen.py VCDS OUT [PER] [SEED] [TRACES]: PER questions a subcategory, each on another of TRACES
    (comma-separated names without .vcd; default the eight of the dev set). SEED 7 is the dev set."""
    vcds, out = sys.argv[1], sys.argv[2]
    per = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 7
    traces = sys.argv[5].split(",") if len(sys.argv) > 5 else TRACES
    # subcategory i's j-th question is on trace (i + j): each subcategory on `per` traces
    plan = [(i, j, traces[(i + j) % len(traces)]) for i in range(len(SUBCATEGORIES)) for j in range(per)]
    rows = []
    for name in traces:
        todo = [(i, j) for i, j, n in plan if n == name]
        if not todo:
            continue
        print("reading", name, file=sys.stderr)
        tr = Trace(name, *load(f"{vcds}/{name}.vcd", name))  # one trace in memory at a time
        for i, j in todo:
            cat, sub, f = SUBCATEGORIES[i]
            r = random.Random(f"{seed}-{sub}-{j}")
            got = None
            for _ in range(0 if sub in ("next_state", "state_sequence", "return_count") and not tr.fsm else 5000):
                got = f(tr, r)
                if got:
                    break
            if not got:
                print(f"no question: {sub} on {name}", file=sys.stderr)
                continue
            q, a, kind = got
            rows.append({"id": f"{sub}-{j}", "category": cat, "subcategory": sub, "trace": name + ".vcd", "question": q, "answer": a, "kind": kind})
        del tr
    order = {(s[1]): k for k, s in enumerate(SUBCATEGORIES)}
    rows.sort(key=lambda r: (order[r["subcategory"]], r["id"]))
    with open(out, "w") as f:
        for row in rows:
            f.write(json.dumps(row) + "\n")
    print(len(rows), "questions", file=sys.stderr)


if __name__ == "__main__":
    main()
