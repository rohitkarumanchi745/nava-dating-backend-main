#!/usr/bin/env python3
"""GNN trainer for user graph embeddings.

Pipeline:
  1. GET  /admin/gnn/edges   -> pull positive interaction edges (paginated)
  2. build the user-interaction graph
  3. train LightGCN (PyTorch Geometric) with BPR loss
  4. POST /admin/gnn/embeddings -> write per-user embeddings back

The embeddings are served cheaply by the Rust backend as a pairwise score
blended into the reciprocal matcher (gated by GNN_SCORE_WEIGHT). Training is
offline and periodic; it stops when done. Nothing here is in the request path.

Run:
  ADMIN_TOKEN=... API_BASE=https://api.nava.app python3 gnn_trainer.py

Requires (training host, ideally GPU):
  pip install torch torch_geometric requests

NOTE: PyTorch Geometric's LightGCN API varies across versions; the methods used
below (`recommendation_loss`, `get_embedding`, forward with `edge_label_index`)
match recent PyG. Reconcile names if you pin a different version.
"""

import os
import sys
from typing import Dict, List, Tuple

import requests

API_BASE = os.environ.get("API_BASE", "http://localhost:8080")
ADMIN_TOKEN = os.environ.get("ADMIN_TOKEN", "")
EMB_DIM = int(os.environ.get("GNN_EMB_DIM", "64"))
NUM_LAYERS = int(os.environ.get("GNN_LAYERS", "3"))
EPOCHS = int(os.environ.get("GNN_EPOCHS", "50"))
LR = float(os.environ.get("GNN_LR", "1e-3"))
BATCH = int(os.environ.get("GNN_BATCH", "8192"))
MODEL_VERSION = int(os.environ.get("GNN_MODEL_VERSION", "1"))
UPLOAD_BATCH = int(os.environ.get("GNN_UPLOAD_BATCH", "2000"))

HEADERS = {"Authorization": f"Bearer {ADMIN_TOKEN}", "Content-Type": "application/json"}


# ---------------------------------------------------------------------------
# 1. Pull edges
# ---------------------------------------------------------------------------
def fetch_edges() -> List[Tuple[int, int]]:
    edges: List[Tuple[int, int]] = []
    since = 0
    while True:
        r = requests.get(
            f"{API_BASE}/admin/gnn/edges",
            headers=HEADERS,
            params={"since_id": since, "limit": 50000},
            timeout=60,
        )
        r.raise_for_status()
        data = r.json()
        batch = data.get("edges", [])
        if not batch:
            break
        edges.extend((e["src"], e["dst"]) for e in batch)
        nxt = data.get("next_since_id", since)
        if nxt <= since:
            break
        since = nxt
    return edges


# ---------------------------------------------------------------------------
# 2/3. Build graph + train LightGCN
# ---------------------------------------------------------------------------
def train(edges: List[Tuple[int, int]]) -> Dict[int, List[float]]:
    import torch
    from torch_geometric.nn import LightGCN

    if not edges:
        return {}

    # Contiguous node indexing.
    users = sorted({u for e in edges for u in e})
    idx = {u: i for i, u in enumerate(users)}
    num_nodes = len(users)

    src = torch.tensor([idx[s] for s, _ in edges], dtype=torch.long)
    dst = torch.tensor([idx[d] for _, d in edges], dtype=torch.long)
    edge_index = torch.stack([src, dst], dim=0)

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    edge_index = edge_index.to(device)
    model = LightGCN(num_nodes=num_nodes, embedding_dim=EMB_DIM, num_layers=NUM_LAYERS).to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=LR)
    num_edges = edge_index.size(1)

    model.train()
    for epoch in range(EPOCHS):
        perm = torch.randperm(num_edges, device=device)
        total = 0.0
        for start in range(0, num_edges, BATCH):
            batch = perm[start:start + BATCH]
            pos = edge_index[:, batch]
            # Negative sampling: random destination per positive edge.
            neg_dst = torch.randint(0, num_nodes, (pos.size(1),), device=device)
            edge_label_index = torch.cat(
                [pos, torch.stack([pos[0], neg_dst], dim=0)], dim=1
            )
            optimizer.zero_grad()
            rank = model(edge_index, edge_label_index)
            pos_rank, neg_rank = rank.chunk(2)
            loss = model.recommendation_loss(pos_rank, neg_rank, node_id=edge_label_index.unique())
            loss.backward()
            optimizer.step()
            total += float(loss)
        print(f"epoch {epoch + 1}/{EPOCHS} loss={total:.4f}", file=sys.stderr)

    model.eval()
    with torch.no_grad():
        emb = model.get_embedding(edge_index).cpu()
    return {users[i]: emb[i].tolist() for i in range(num_nodes)}


# ---------------------------------------------------------------------------
# 4. Write embeddings back
# ---------------------------------------------------------------------------
def upload(embeddings: Dict[int, List[float]]) -> None:
    items = [{"user_id": uid, "embedding": vec} for uid, vec in embeddings.items()]
    for start in range(0, len(items), UPLOAD_BATCH):
        chunk = items[start:start + UPLOAD_BATCH]
        r = requests.post(
            f"{API_BASE}/admin/gnn/embeddings",
            headers=HEADERS,
            json={"model_version": MODEL_VERSION, "embeddings": chunk},
            timeout=120,
        )
        r.raise_for_status()
        print(f"uploaded {start + len(chunk)}/{len(items)}", file=sys.stderr)


def main() -> None:
    if not ADMIN_TOKEN:
        sys.exit("ADMIN_TOKEN is required")
    edges = fetch_edges()
    print(f"fetched {len(edges)} edges", file=sys.stderr)
    embeddings = train(edges)
    if not embeddings:
        print("no embeddings produced (no edges yet)", file=sys.stderr)
        return
    upload(embeddings)
    print(f"done: {len(embeddings)} user embeddings, model_version={MODEL_VERSION}")


if __name__ == "__main__":
    main()
