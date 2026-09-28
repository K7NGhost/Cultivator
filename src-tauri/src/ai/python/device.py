"""Chooses where a local AI model runs.

Prepended to each model worker script at build time (see ai/*.rs). An
NVIDIA GPU is used when the installed PyTorch was built with CUDA for it;
otherwise, or if the GPU fails a quick self-test, the CPU. Setting
CULTIVATOR_AI_DEVICE=cpu forces the CPU.
"""

import os
import sys

# Each model needs well under this at half precision (twice it at float32); a
# card with less free memory (or already busy with another model) keeps that
# model on the CPU.
MIN_FREE_GPU_BYTES = 1536 * 1024 * 1024


def pick_device(torch, cpu_bfloat16=False):
    """Returns (device, dtype, human-readable device name).

    With `cpu_bfloat16`, a CPU with native bfloat16 instructions (AVX512-BF16
    or AMX) runs the model in bfloat16: measured 1.9x faster for BGE-M3 with
    vectors 99.99% identical. Captioning doesn't ask for it, because BLIP's
    captions change in bfloat16.
    """
    if os.environ.get("CULTIVATOR_AI_DEVICE", "").lower() != "cpu" and torch.cuda.is_available():
        try:
            probe = torch.ones((8, 8), device="cuda")
            # Fails fast when this GPU has no kernels in the installed build.
            (probe @ probe).sum().item()
            # GPUs before Volta (2017), like the GTX 10-series and Quadro P
            # cards common in labs, run half precision far slower than float32.
            major, _minor = torch.cuda.get_device_capability(0)
            dtype = torch.float16 if major >= 7 else torch.float32
            needed = MIN_FREE_GPU_BYTES * (1 if dtype == torch.float16 else 2)
            free, _total = torch.cuda.mem_get_info()
            if free >= needed:
                return "cuda", dtype, torch.cuda.get_device_name(0)
            print(f"Only {free // 2**20} MB of GPU memory free, using the CPU instead.", file=sys.stderr)
        except Exception as error:  # noqa: BLE001 - any GPU failure falls back to CPU
            print(f"GPU unavailable, using the CPU instead: {error}", file=sys.stderr)
    threads = torch.get_num_threads()
    if cpu_bfloat16 and native_bfloat16(torch):
        return "cpu", torch.bfloat16, f"CPU ({threads} threads, bfloat16)"
    return "cpu", torch.float32, f"CPU ({threads} threads)"


def native_bfloat16(torch):
    """Emulated bfloat16 is slower than float32, so only native support counts."""
    for probe in ("_is_avx512_bf16_supported", "_is_amx_tile_supported"):
        try:
            if getattr(torch.cpu, probe)():
                return True
        except Exception:  # noqa: BLE001 - a private PyTorch probe; missing means no
            pass
    return False
