"""A small VCD reader, the referee: the ground truth comes from it, never from wtf."""


class Signal:
    def __init__(self, path, width):
        self.path, self.width = path, width
        self.changes = []  # (time, value): value an int, or None for x/z; the last change at a time wins

    def add(self, t, v):
        if self.changes and self.changes[-1][0] == t:
            self.changes.pop()
            if self.changes and self.changes[-1][1] == v:
                return
        if not self.changes or self.changes[-1][1] != v:
            self.changes.append((t, v))

    def at(self, t):
        """The value after every change at t."""
        lo, hi = 0, len(self.changes)
        while lo < hi:
            mid = (lo + hi) // 2
            if self.changes[mid][0] <= t:
                lo = mid + 1
            else:
                hi = mid
        return self.changes[lo - 1][1] if lo else None

    def before(self, t):
        """The value just before t."""
        return self.at(t - 1)

    def times(self):
        return [t for t, _ in self.changes]

    def changes_at(self, t):
        return self.at(t) != self.before(t)

    def edges(self, rising=True):
        key = "_rise" if rising else "_fall"
        if key not in self.__dict__:
            self.__dict__[key] = self._edges(rising)
        return self.__dict__[key]

    def _edges(self, rising):
        out, prev = [], None
        for t, v in self.changes:
            if prev is not None and (prev == 0 and v == 1 if rising else prev == 1 and v == 0):
                out.append(t)
            prev = v
        return out


def value(tok):
    s = tok.lower()
    if any(c in s for c in "xzu-"):
        return None
    return int(s, 2)


def read(path, wanted=None):
    """Signals by path (scopes joined with '.'); `wanted`, a function on (path, width), keeps fewer."""
    sigs, by_id, scope, unit, end = {}, {}, [], "", 0
    with open(path, "r", errors="replace") as f:
        header = True
        t = 0
        for line in f:
            if header:
                s = line.strip()
                if s.startswith("$timescale"):
                    rest = s[len("$timescale"):].replace("$end", "").strip() or next(f).strip()
                    unit = rest.replace("$end", "").strip()
                elif s.startswith("$scope"):
                    scope.append(s.split()[2])
                elif s.startswith("$upscope"):
                    scope.pop()
                elif s.startswith("$var"):
                    p = s.split()
                    width, code, name = int(p[2]), p[3], p[4]
                    full = ".".join(scope + [name])
                    if wanted is None or wanted(full, width):
                        sig = sigs.setdefault(full, Signal(full, width))
                        by_id.setdefault(code, []).append(sig)
                elif s.startswith("$enddefinitions"):
                    header = False
                continue
            s = line.strip()
            if not s or s[0] == "$":
                continue
            c = s[0]
            if c == "#":
                t = int(s[1:])
                end = t
            elif c in "bB":
                parts = s[1:].split()
                if len(parts) != 2:
                    continue  # a trace cut off mid-line
                v, code = parts
                for sig in by_id.get(code, ()):
                    sig.add(t, value(v))
            elif c in "rR":
                continue
            else:
                for sig in by_id.get(s[1:], ()):
                    sig.add(t, value(c))
    return sigs, unit, end


def header(path):
    """(path, width) of every signal, without reading the changes."""
    out, scope = [], []
    with open(path, "r", errors="replace") as f:
        for line in f:
            s = line.strip()
            if s.startswith("$scope"):
                scope.append(s.split()[2])
            elif s.startswith("$upscope"):
                scope.pop()
            elif s.startswith("$var"):
                p = s.split()
                out.append((".".join(scope + [p[4]]), int(p[2])))
            elif s.startswith("$enddefinitions"):
                break
    return out
