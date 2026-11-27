#!/usr/bin/env python3
"""Per-lane self/incl time of scopes inside a time window of a Chrome-trace export
(rustudio --profile-convert X.rsprof out.json). Usage: trace-window.py FILE T0 T1 [TOP] [LANE-SUBSTR]
T0/T1 are seconds from the capture's first event."""
import json, sys, collections
f, t0, t1 = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
top = int(sys.argv[4]) if len(sys.argv) > 4 else 25
lane_filter = sys.argv[5] if len(sys.argv) > 5 else None
d = json.load(open(f))
ev = d['traceEvents']
names = {}
for e in ev:
    if e.get('ph') == 'M' and e.get('name') == 'thread_name':
        names[e['tid']] = e['args']['name']
base = min(e['ts'] for e in ev if e.get('ph') == 'X')
lo, hi = base + t0 * 1e6, base + t1 * 1e6
by_lane = collections.defaultdict(list)
for e in ev:
    if e.get('ph') != 'X':
        continue
    s, dur = e['ts'], e.get('dur', 0)
    if s < lo or s + dur > hi:
        continue
    by_lane[e['tid']].append((s, s + dur, e['name']))
print(f"window {t0}-{t1} s ({(t1-t0):.1f} s)")
for tid, spans in sorted(by_lane.items(), key=lambda kv: -len(kv[1])):
    lane = names.get(tid, str(tid))
    if lane_filter and lane_filter not in lane:
        continue
    if lane == 'Frames':
        # frame markers: count per category
        cats = collections.Counter(n.split(' ')[0] for _, _, n in spans)
        durs = collections.defaultdict(list)
        for s, e2, n in spans:
            durs[n.split(' ')[0]].append((e2 - s) / 1000)
        for k, v in durs.items():
            v.sort()
            print(f"  frames {k}: n={len(v)} ({len(v)/(t1-t0):.1f}/s) mean={sum(v)/len(v):.1f} p50={v[len(v)//2]:.1f} p95={v[int(len(v)*0.95)]:.1f} max={v[-1]:.1f} ms")
        continue
    # self time: subtract children (spans nested within on the same lane)
    spans.sort(key=lambda x: (x[0], -x[1]))
    incl = collections.Counter(); self_ = collections.Counter(); calls = collections.Counter()
    stack = []
    for s, e2, n in spans:
        while stack and stack[-1][1] <= s:
            stack.pop()
        dur = e2 - s
        incl[n] += dur; self_[n] += dur; calls[n] += 1
        if stack:
            self_[stack[-1][2]] -= dur
        stack.append((s, e2, n))
    busy = sum(self_.values()) / 1000
    print(f"\n== lane {lane}: {len(spans)} spans, busy {busy:.0f} ms = {busy/(t1-t0)/10:.1f} % of the window")
    print(f"  {'scope':40s} {'self ms':>9s} {'incl ms':>9s} {'calls':>6s} {'self/call':>9s}")
    for n, v in sorted(self_.items(), key=lambda kv: -kv[1])[:top]:
        print(f"  {n[:40]:40s} {v/1000:9.1f} {incl[n]/1000:9.1f} {calls[n]:6d} {v/1000/max(calls[n],1):9.3f}")
