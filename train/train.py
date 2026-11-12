"""Fit the value network on positions written by `gen_data`.

Every stored position is one option of one decision, next to how the hand ended when that option
was played on the real deal. Options of the same decision form a group. What the network must get
right is the difference between the options of a group, so the loss is split: differences inside a
group count fully, the level of the whole group (which is mostly luck of the deal) counts `--level`.

    python train/train.py --data data/gen0 --out data/gen0.bin
"""

import argparse
import glob
import itertools
import math
import struct
import time

import numpy as np
import torch
from torch import nn

PLANES = 13
NIDS = 53
NBITS_PLAIN = 804
NREAL_PLAIN = 23


def load(dirs, limit):
    parts, meta = [], None
    for path in sorted(p for d in dirs for p in glob.glob(d + "/*.bin")):
        with open(path, "rb") as f:
            head = f.read(24)
        magic, layout, nbits, nreal, size, partners = struct.unpack("<4sIIIII", head)
        assert magic == b"OKDT" and layout == 3, path
        assert meta in (None, (layout, nbits, nreal, size, partners)), "shards disagree on the feature layout or the kind of game"
        meta = (layout, nbits, nreal, size, partners)
        raw = np.fromfile(path, dtype=np.uint8, offset=24)
        parts.append(raw[: len(raw) // size * size].reshape(-1, size))
    data = np.concatenate(parts)
    if limit:
        data = data[:limit]
    return data, meta


def plane_starts(nbits):
    return [k * NIDS for k in range(PLANES)] + list(range(NBITS_PLAIN, nbits, NIDS))


def colour_shuffles(nbits, nreal, device):
    out = []
    for perm in itertools.permutations(range(4)):
        idx = np.arange(nbits + nreal)
        for start in plane_starts(nbits):
            for c in range(4):
                for n in range(13):
                    idx[start + c * 13 + n] = start + perm[c] * 13 + n
        out.append(torch.from_numpy(idx).to(device))
    return out


class Batches:
    def __init__(self, data, meta, device, plain=False, ram=False):
        self.layout, self.nbits, self.nreal, self.size, self.partners = meta
        nbytes = (self.nbits + 63) // 64 * 8
        self.cut = (nbytes, nbytes + self.nreal)
        y = data[:, self.cut[1] : self.cut[1] + 4].copy().view("<f4")[:, 0]
        flags = data[:, self.cut[1] + 4]
        starts = np.flatnonzero(flags & 1)
        self.sizes = torch.from_numpy(np.diff(np.append(starts, len(data)))).to(device)
        self.starts = torch.from_numpy(starts).to(device)
        self.draw = torch.from_numpy((flags[starts] & 2) != 0).to(device)
        self.x = torch.from_numpy(np.ascontiguousarray(data[:, : self.cut[1]]))
        if not ram:
            self.x = self.x.to(device)
        self.plain = plain and self.nbits > NBITS_PLAIN
        self.file_nbits = self.nbits
        if self.plain:
            self.nbits, self.nreal = NBITS_PLAIN, NREAL_PLAIN
        self.y = torch.from_numpy(y.astype(np.float32)).to(device)
        self.shifts = torch.arange(8, device=device, dtype=torch.uint8)
        self.device = device

    def take(self, groups):
        n = self.sizes[groups]
        gi = torch.repeat_interleave(torch.arange(len(groups), device=self.device), n)
        first = torch.cumsum(n, 0) - n
        idx = self.starts[groups][gi] + torch.arange(len(gi), device=self.device) - first[gi]
        rec = self.x[idx] if self.x.is_cuda else self.x[idx.cpu()].to(self.device)
        bits = ((rec[:, : self.cut[0]].unsqueeze(-1) >> self.shifts) & 1).reshape(len(idx), -1)[:, : self.nbits]
        x = torch.cat([bits.float(), rec[:, self.cut[0] : self.cut[0] + self.nreal].float() / 255.0], 1)
        return x, self.y[idx], gi, n.float()


def split_loss(f, y, gi, n):
    zeros = torch.zeros(len(n), device=f.device)
    mf = zeros.index_add(0, gi, f) / n
    my = zeros.index_add(0, gi, y) / n
    inside = ((f - mf[gi]) - (y - my[gi])) ** 2
    return inside.mean(), ((mf - my) ** 2).mean(), my


def judge(model, b, groups, batch):
    model.eval()
    tot = dict(inside=0.0, spread=0.0, level=0.0, level_spread=0.0, pick=0.0, best=0.0, n=0, g=0, take_right=0.0, take_n=0)
    with torch.no_grad():
        for i in range(0, len(groups), batch):
            sel = groups[i : i + batch]
            x, y, gi, n = b.take(sel)
            f = model(x).squeeze(1)
            inside, level, my = split_loss(f, y, gi, n)
            tot["inside"] += inside.item() * len(y)
            tot["spread"] += ((y - my[gi]) ** 2).sum().item()
            tot["level"] += level.item() * len(sel)
            tot["level_spread"] += (my**2).sum().item()
            big = torch.full((len(sel),), float("inf"), device=f.device)
            fmin = big.scatter_reduce(0, gi, f, "amin")
            picked = (f == fmin[gi]).float()
            pick_y = torch.zeros_like(big).index_add(0, gi, y * picked) / torch.zeros_like(big).index_add(0, gi, picked)
            ymin = big.scatter_reduce(0, gi, y, "amin")
            tot["pick"] += (pick_y - my).sum().item()
            tot["best"] += (ymin - my).sum().item()
            tot["n"] += len(y)
            tot["g"] += len(sel)
    model.train()
    return dict(
        inside=tot["inside"] / tot["n"],
        explained=1 - tot["inside"] / tot["spread"],
        level=tot["level"] / tot["g"],
        level_explained=1 - tot["level"] / tot["level_spread"],
        pick=tot["pick"] / tot["g"],
        best=tot["best"] / tot["g"],
    )


def export(model, path, b, scale):
    # Must match Net::from_bytes in crates/engine/src/net.rs.
    layers = [m for m in model if isinstance(m, nn.Linear)]
    with open(path, "wb") as f:
        f.write(b"OKNN")
        f.write(struct.pack("<IIIIfI", b.layout, b.nbits, b.nreal, b.partners, scale, len(layers)))
        for m in layers:
            w = m.weight.detach().cpu().numpy().astype("<f4")
            f.write(struct.pack("<II", w.shape[1], w.shape[0]))
            f.write(w.tobytes())
            f.write(m.bias.detach().cpu().numpy().astype("<f4").tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", nargs="+", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--hidden", default="256,128,64")
    ap.add_argument("--epochs", type=float, default=12)
    ap.add_argument("--batch", type=int, default=4096, help="decisions per step")
    ap.add_argument("--lr", type=float, default=1.5e-3)
    ap.add_argument("--level", type=float, default=0.1, help="weight of a group's overall level")
    ap.add_argument("--limit", type=int, default=0, help="use only this many positions")
    ap.add_argument("--init", default="", help="start from a saved model (.pt)")
    ap.add_argument("--scale", type=float, default=300.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--ram", action="store_true", help="keep the data in main memory instead of on the graphics card")
    ap.add_argument("--plain", action="store_true", help="leave out the draw-by-draw inputs (a faster network)")
    args = ap.parse_args()

    torch.manual_seed(args.seed)
    device = "cuda"
    data, meta = load(args.data, args.limit)
    b = Batches(data, meta, device, args.plain, args.ram)
    del data
    groups = torch.arange(len(b.starts), device=device)
    held = max(len(groups) // 50, 1)
    train, val = groups[:-held], groups[-held:]
    print(f"{len(b.y):,} positions in {len(groups):,} decisions ({b.draw.float().mean().item():.1%} draw-or-take), {held:,} held out", flush=True)

    widths = [b.nbits + b.nreal] + [int(h) for h in args.hidden.split(",")] + [1]
    mods = []
    for i in range(len(widths) - 1):
        mods.append(nn.Linear(widths[i], widths[i + 1]))
        if i < len(widths) - 2:
            mods.append(nn.ReLU())
    model = nn.Sequential(*mods).to(device)
    if args.init:
        model.load_state_dict(torch.load(args.init))
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=1e-5)
    steps = int(args.epochs * len(train) / args.batch)
    warm = min(500, steps // 20)
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: (s + 1) / warm if s < warm else 0.5 * (1 + math.cos(math.pi * (s - warm) / max(steps - warm, 1))))
    shuffles = colour_shuffles(b.nbits, b.nreal, device)

    start, order, at, seen = time.time(), torch.randperm(len(train), device=device), 0, [0.0, 0.0, 0]
    for step in range(steps):
        if at + args.batch > len(order):
            order, at = torch.randperm(len(train), device=device), 0
        x, y, gi, n = b.take(train[order[at : at + args.batch]])
        at += args.batch
        x = x[:, shuffles[torch.randint(len(shuffles), (1,)).item()]]
        inside, level, _ = split_loss(model(x).squeeze(1), y, gi, n)
        loss = inside + args.level * level
        opt.zero_grad(set_to_none=True)
        loss.backward()
        nn.utils.clip_grad_norm_(model.parameters(), 5.0)
        opt.step()
        sched.step()
        seen = [seen[0] + inside.item(), seen[1] + level.item(), seen[2] + 1]
        if (step + 1) % max(steps // 12, 1) == 0 or step + 1 == steps:
            j = judge(model, b, val, args.batch)
            print(
                f"step {step + 1:>6}/{steps}  train inside {seen[0] / seen[2]:.4f} level {seen[1] / seen[2]:.4f} | "
                f"held-out inside {j['inside']:.4f} (explains {j['explained']:.1%}) level {j['level']:.4f} (explains {j['level_explained']:.1%}) | "
                f"pick {j['pick'] * args.scale / 3:+.2f} pts vs average option, hindsight best {j['best'] * args.scale / 3:+.2f} | {time.time() - start:.0f}s",
                flush=True,
            )
            seen = [0.0, 0.0, 0]
    torch.save(model.state_dict(), args.out + ".pt")
    export(model, args.out, b, args.scale)

    # Read by examples/net_check.rs: the positions, then the answers.
    x, _, _, _ = b.take(val[:64])
    with torch.no_grad():
        f = model.eval()(x).squeeze(1) * args.scale
    with open(args.out + ".check", "wb") as out:
        k = len(f)
        first = b.starts[val[0]].item()
        out.write(b.x[first : first + k].cpu().numpy().tobytes())
        out.write(f.cpu().numpy().astype("<f4").tobytes())
    print("saved", args.out)


if __name__ == "__main__":
    main()
