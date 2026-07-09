#!/usr/bin/env python3
"""FedLoRA training worker for per-user chat-suggestion adapters.

Pipeline per job (claimed from the backend):
  1. GET  /admin/lora/jobs/next        -> claim a job + the user's signals
  2. aggregate signals (FedAvg over LoRA deltas, or gather DP-protected examples)
  3. train a small LoRA on the frozen base model (PEFT)
  4. export the adapter to GGUF (llama.cpp convert_lora_to_gguf.py)
  5. upload to object storage -> (url, sha256, size)
  6. POST /admin/lora/adapter          -> register the new version (activates it)
     or POST /admin/lora/jobs/{id}/fail on error

Privacy: signals are DP-protected on the device (their existing FL/DP setup).
This worker never sees a plaintext conversation stream — only per-job signal.

This is the server half of the "hybrid" design: the phone contributes signal and
runs inference-with-adapter; the heavy LoRA training happens here.

Run:
  ADMIN_TOKEN=... API_BASE=https://api.nava.app \
  BASE_MODEL=/models/bitnet-b1.58-2B-4T \
  LLAMA_CPP=/opt/llama.cpp \
  python3 fedlora_trainer.py --loop

Requires (training host, ideally GPU):
  pip install torch transformers peft datasets requests boto3
  a local copy of llama.cpp (for convert_lora_to_gguf.py)
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Tuple

import requests

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
API_BASE = os.environ.get("API_BASE", "http://localhost:8080")
ADMIN_TOKEN = os.environ.get("ADMIN_TOKEN", "")
BASE_MODEL = os.environ.get("BASE_MODEL", "/models/bitnet-b1.58-2B-4T")
LLAMA_CPP = os.environ.get("LLAMA_CPP", "/opt/llama.cpp")
LORA_RANK = int(os.environ.get("LORA_RANK", "8"))
LORA_ALPHA = int(os.environ.get("LORA_ALPHA", "16"))
EPOCHS = int(os.environ.get("LORA_EPOCHS", "2"))
LR = float(os.environ.get("LORA_LR", "2e-4"))
DP_NOISE = float(os.environ.get("DP_NOISE_MULTIPLIER", "1.0"))
DP_CLIP = float(os.environ.get("DP_CLIP_NORM", "1.0"))
POLL_SECONDS = int(os.environ.get("POLL_SECONDS", "30"))

HEADERS = {"Authorization": f"Bearer {ADMIN_TOKEN}", "Content-Type": "application/json"}


# ---------------------------------------------------------------------------
# Backend API
# ---------------------------------------------------------------------------
def claim_job() -> Optional[Dict[str, Any]]:
    r = requests.get(f"{API_BASE}/admin/lora/jobs/next", headers=HEADERS, timeout=30)
    r.raise_for_status()
    return r.json().get("job")


def register_adapter(job: Dict[str, Any], url: str, sha256: str, size: int,
                     metrics: Dict[str, Any]) -> None:
    body = {
        "job_id": job["job_id"],
        "user_id": job["user_id"],
        "version": job["round"],
        "storage_url": url,
        "sha256": sha256,
        "size_bytes": size,
        "base_model": os.path.basename(BASE_MODEL.rstrip("/")),
        "rank": LORA_RANK,
        "metrics": metrics,
    }
    r = requests.post(f"{API_BASE}/admin/lora/adapter", headers=HEADERS,
                      data=json.dumps(body), timeout=60)
    r.raise_for_status()


def fail_job(job_id: int, error: str) -> None:
    requests.post(f"{API_BASE}/admin/lora/jobs/{job_id}/fail", headers=HEADERS,
                  data=json.dumps({"error": error[:500]}), timeout=30)


# ---------------------------------------------------------------------------
# Signal aggregation
# ---------------------------------------------------------------------------
def gather_examples(signals: List[Dict[str, Any]]) -> List[Dict[str, str]]:
    """Flatten DP-protected training examples from all of a user's signals.

    Expected per-signal shape (produced on-device):
        {"examples": [{"prompt": "...", "completion": "..."}, ...]}
    """
    examples: List[Dict[str, str]] = []
    for s in signals:
        for ex in s.get("examples", []):
            p, c = ex.get("prompt"), ex.get("completion")
            if p and c:
                examples.append({"prompt": p, "completion": c})
    return examples


def has_deltas(signals: List[Dict[str, Any]]) -> bool:
    return any("lora_delta" in s for s in signals)


def fedavg_deltas(signals: List[Dict[str, Any]]):
    """FedAvg over client-supplied LoRA deltas, with DP clip + Gaussian noise.

    Each signal carries {"lora_delta": {param_name: [floats]}, "num_samples": n}.
    Returns {param_name: torch.Tensor} to merge into the adapter.
    """
    import torch  # local import so the API-only paths don't need torch

    agg: Dict[str, torch.Tensor] = {}
    total = 0
    for s in signals:
        delta = s.get("lora_delta")
        if not delta:
            continue
        n = max(1, int(s.get("num_samples", 1)))
        total += n
        for name, flat in delta.items():
            t = torch.tensor(flat, dtype=torch.float32)
            # Per-client L2 clip (DP sensitivity bound).
            norm = t.norm()
            if norm > DP_CLIP:
                t = t * (DP_CLIP / norm)
            agg[name] = agg.get(name, torch.zeros_like(t)) + t * n
    if total == 0:
        return {}
    for name in agg:
        agg[name] /= total
        # Gaussian DP noise.
        agg[name] += torch.randn_like(agg[name]) * (DP_NOISE * DP_CLIP / total)
    return agg


# ---------------------------------------------------------------------------
# Training
# ---------------------------------------------------------------------------
def train_lora_sft(examples: List[Dict[str, str]], out_dir: str) -> Dict[str, Any]:
    """Supervised fine-tune a LoRA adapter on the user's own examples."""
    import torch
    from datasets import Dataset
    from peft import LoraConfig, get_peft_model
    from transformers import (AutoModelForCausalLM, AutoTokenizer,
                              DataCollatorForLanguageModeling, Trainer,
                              TrainingArguments)

    tok = AutoTokenizer.from_pretrained(BASE_MODEL)
    if tok.pad_token is None:
        tok.pad_token = tok.eos_token
    model = AutoModelForCausalLM.from_pretrained(BASE_MODEL, torch_dtype=torch.float32)

    lora = LoraConfig(
        r=LORA_RANK, lora_alpha=LORA_ALPHA, lora_dropout=0.05, bias="none",
        task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj"],
    )
    model = get_peft_model(model, lora)

    def fmt(ex):
        text = f"{ex['prompt']}\n{ex['completion']}{tok.eos_token}"
        return tok(text, truncation=True, max_length=512)

    ds = Dataset.from_list(examples).map(fmt, remove_columns=["prompt", "completion"])
    args = TrainingArguments(
        output_dir=out_dir, num_train_epochs=EPOCHS, learning_rate=LR,
        per_device_train_batch_size=4, gradient_accumulation_steps=2,
        logging_steps=10, save_strategy="no", report_to=[],
    )
    trainer = Trainer(
        model=model, args=args, train_dataset=ds,
        data_collator=DataCollatorForLanguageModeling(tok, mlm=False),
    )
    result = trainer.train()
    model.save_pretrained(out_dir)
    return {"train_loss": float(result.training_loss), "examples": len(examples)}


# ---------------------------------------------------------------------------
# Export + upload
# ---------------------------------------------------------------------------
def export_to_gguf(adapter_dir: str, out_path: str) -> None:
    """Convert a PEFT LoRA adapter to GGUF for llama.cpp / bitnet.cpp."""
    converter = os.path.join(LLAMA_CPP, "convert_lora_to_gguf.py")
    subprocess.run(
        [sys.executable, converter, adapter_dir, "--base", BASE_MODEL,
         "--outfile", out_path, "--outtype", "f16"],
        check=True,
    )


def sha256_of(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def upload(path: str, user_id: int, version: int) -> str:
    """Upload the GGUF adapter and return its download URL.

    Defaults to S3 if S3_BUCKET is set; otherwise copies to LOCAL_ADAPTER_DIR
    (dev) and returns a PUBLIC_ADAPTER_BASE URL.
    """
    key = f"adapters/{user_id}/v{version}.gguf"
    bucket = os.environ.get("S3_BUCKET")
    if bucket:
        import boto3
        boto3.client("s3").upload_file(path, bucket, key)
        base = os.environ.get("PUBLIC_ADAPTER_BASE", f"https://{bucket}.s3.amazonaws.com")
        return f"{base}/{key}"

    local_dir = os.environ.get("LOCAL_ADAPTER_DIR", "/tmp/nava-adapters")
    dst = os.path.join(local_dir, key)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    subprocess.run(["cp", path, dst], check=True)
    return f"{os.environ.get('PUBLIC_ADAPTER_BASE', 'http://localhost:9000')}/{key}"


# ---------------------------------------------------------------------------
# Job runner
# ---------------------------------------------------------------------------
def run_job(job: Dict[str, Any]) -> None:
    job_id, user_id, version = job["job_id"], job["user_id"], job["round"]
    signals = job.get("signals", [])
    print(f"[job {job_id}] user={user_id} v{version} signals={len(signals)}")

    with tempfile.TemporaryDirectory() as work:
        adapter_dir = os.path.join(work, "adapter")
        os.makedirs(adapter_dir, exist_ok=True)

        # Mode A: on-device deltas -> FedAvg. Mode B: examples -> server SFT.
        if has_deltas(signals):
            merged = fedavg_deltas(signals)
            _save_delta_adapter(merged, adapter_dir)
            metrics = {"mode": "fedavg", "clients": len(signals)}
        else:
            examples = gather_examples(signals)
            if len(examples) < 5:
                raise ValueError(f"too few examples ({len(examples)})")
            metrics = {"mode": "sft", **train_lora_sft(examples, adapter_dir)}

        gguf = os.path.join(work, f"user-{user_id}-v{version}.gguf")
        export_to_gguf(adapter_dir, gguf)

        url = upload(gguf, user_id, version)
        register_adapter(job, url, sha256_of(gguf), os.path.getsize(gguf), metrics)
        print(f"[job {job_id}] registered v{version} -> {url}")


def _save_delta_adapter(merged, adapter_dir: str) -> None:
    """Write FedAvg-merged deltas into a PEFT-loadable adapter directory."""
    import torch
    from safetensors.torch import save_file
    save_file(merged, os.path.join(adapter_dir, "adapter_model.safetensors"))
    cfg = {"peft_type": "LORA", "r": LORA_RANK, "lora_alpha": LORA_ALPHA,
           "target_modules": ["q_proj", "k_proj", "v_proj", "o_proj"],
           "task_type": "CAUSAL_LM", "base_model_name_or_path": BASE_MODEL}
    with open(os.path.join(adapter_dir, "adapter_config.json"), "w") as f:
        json.dump(cfg, f)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--loop", action="store_true", help="poll continuously")
    args = ap.parse_args()

    if not ADMIN_TOKEN:
        sys.exit("ADMIN_TOKEN is required")

    while True:
        try:
            job = claim_job()
        except Exception as e:  # noqa: BLE001
            print(f"claim failed: {e}", file=sys.stderr)
            job = None

        if job is None:
            if not args.loop:
                print("no pending jobs")
                return
            time.sleep(POLL_SECONDS)
            continue

        try:
            run_job(job)
        except Exception as e:  # noqa: BLE001
            print(f"[job {job.get('job_id')}] FAILED: {e}", file=sys.stderr)
            try:
                fail_job(job["job_id"], str(e))
            except Exception:  # noqa: BLE001
                pass

        if not args.loop:
            return


if __name__ == "__main__":
    main()
