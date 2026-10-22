"""Multiway Bradley-Terry / Luce "top choice" model, fit by the Hunter (2004)
MM algorithm ("MM algorithms for generalized Bradley-Terry models").

Each pick event is one comparison: a set of alternatives (the pack) of which
exactly one (the picked card) "wins". Under the Luce model,
P(i wins | alternatives S) = v_i / sum_{j in S} v_j. The MM fixed point is:

    v_i <- W_i / D_i,   D_i = sum_{events m : i in S_m} 1 / (sum_{j in S_m} v_j)

where W_i is the number of events item i won. This generalizes Zermelo's
pairwise Elo-like MLE to multi-item comparisons directly (a pick from a pack
of 15 is one 15-way comparison, not 14 separate pairwise ones), so results
do not depend on an update order or learning rate the way online Elo does.

Cards that are seen but never picked get v_i -> 0 (W_i = 0); cards that are
always alone in their event are undefined. We add a small ridge
(prior_strength pseudo-wins/pseudo-losses split evenly) to keep every card
with >=1 appearance finite and to regularize thin estimates.
"""
from __future__ import annotations

import numpy as np
import pandas as pd


def fit_pack_bt(
    pack_ids: list[list[int]],
    picked_idx: list[int],
    n_items: int,
    n_iter: int = 60,
    ridge: float = 0.5,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Fit v (length n_items) from ragged pack_ids (lists of item indices,
    0..n_items-1) and the index within each pack of the picked item.

    Returns (v, win_counts, appearance_counts).
    """
    win_counts = np.zeros(n_items, dtype=np.float64)
    appear_counts = np.zeros(n_items, dtype=np.float64)

    # group events by pack size for vectorized (padded) updates
    by_size: dict[int, list[tuple[list[int], int]]] = {}
    for pack, pick_pos in zip(pack_ids, picked_idx):
        by_size.setdefault(len(pack), []).append((pack, pick_pos))

    size_arrays: dict[int, tuple[np.ndarray, np.ndarray]] = {}
    for size, events in by_size.items():
        packs = np.array([e[0] for e in events], dtype=np.int64)  # (n_events, size)
        picks = np.array([e[1] for e in events], dtype=np.int64)  # (n_events,)
        size_arrays[size] = (packs, picks)
        winners = packs[np.arange(len(packs)), picks]
        np.add.at(win_counts, winners, 1.0)
        for col in range(size):
            np.add.at(appear_counts, packs[:, col], 1.0)

    # ridge: add a fictitious event where every seen card ties against a
    # uniform "ghost" opponent, worth `ridge` pseudo-wins and pseudo-losses.
    seen = appear_counts > 0
    v = np.ones(n_items, dtype=np.float64)
    v[~seen] = np.nan

    win_counts_reg = win_counts + ridge
    for _ in range(n_iter):
        denom = np.zeros(n_items, dtype=np.float64)
        for size, (packs, _picks) in size_arrays.items():
            vals = v[packs]  # (n_events, size), may contain nan only if unseen (shouldn't happen: all packs cards are seen)
            event_sum = vals.sum(axis=1)  # (n_events,)
            inv = 1.0 / event_sum
            for col in range(size):
                np.add.at(denom, packs[:, col], inv)
        # ridge ghost opponent: pretend one extra comparison against a fixed-strength
        # ghost (v_ghost = 1) for every seen card, contributing to both win and denom.
        denom_reg = denom + ridge / 1.0
        v_new = np.where(seen, win_counts_reg / np.maximum(denom_reg, 1e-12), np.nan)
        # normalize geometric mean of seen items to 1 to keep scale stable
        gm = np.exp(np.nanmean(np.log(v_new[seen])))
        v_new = v_new / gm
        v = v_new

    return v, win_counts, appear_counts


def fit_from_pick_rows(df: pd.DataFrame, pack_col: str, picked_col: str, n_iter: int = 60, ridge: float = 0.5):
    """df has one row per pick event. pack_col holds a list of card keys
    (oracle_id or name); picked_col holds the picked card's key (must be a
    member of the pack list). Returns a DataFrame indexed by card key with
    columns v (fitted strength), win_count, appear_count.
    """
    vocab = sorted({c for pack in df[pack_col] for c in pack if c is not None})
    key_to_idx = {k: i for i, k in enumerate(vocab)}

    pack_ids = []
    picked_idx = []
    for pack, picked in zip(df[pack_col], df[picked_col]):
        pack = list(pack)
        if picked is None or any(c is None for c in pack):
            continue
        if picked not in pack:
            continue
        idxs = [key_to_idx[c] for c in pack]
        pack_ids.append(idxs)
        picked_idx.append(pack.index(picked))

    v, wins, appears = fit_pack_bt(pack_ids, picked_idx, len(vocab), n_iter=n_iter, ridge=ridge)
    out = pd.DataFrame({"card_key": vocab, "bt_strength": v, "n_picked": wins, "n_seen": appears})
    return out.set_index("card_key")
