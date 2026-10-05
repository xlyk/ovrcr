#!/usr/bin/env python3
"""Generate three original PCM notification tones; never play audio."""

import hashlib
import json
import math
import struct
import sys
import wave
from pathlib import Path

RATE = 48_000
PEAK = 0.18
TONES = (
    ("tap", "Tap", 220, 0.006, 0.040, 6.0,
     "780 Hz with a quieter 1560 Hz harmonic; quick attack and decay."),
    ("chime", "Chime", 840, 0.010, 0.120, 3.0,
     "1046.5, 1569.75 and 2093 Hz partials; a longer decaying chord."),
    ("rise", "Rise", 640, 0.012, 0.080, 0.9,
     "A continuous 660 to 990 Hz glide with a quieter second harmonic."),
)


def samples(tone):
    tone_id, _, milliseconds, attack, release, decay, _ = tone
    count = RATE * milliseconds // 1000
    end = (count - 1) / RATE
    values = []
    for i in range(count):
        t = i / RATE
        ramp_in = math.sin(math.pi / 2 * min(t / attack, 1)) ** 2
        ramp_out = math.sin(math.pi / 2 * min((end - t) / release, 1)) ** 2
        envelope = ramp_in * ramp_out * math.exp(-decay * t / end)
        if tone_id == "tap":
            value = math.sin(2 * math.pi * 780 * t)
            value += 0.20 * math.sin(2 * math.pi * 1560 * t)
        elif tone_id == "chime":
            value = math.sin(2 * math.pi * 1046.5 * t)
            value += 0.32 * math.sin(2 * math.pi * 1569.75 * t)
            value += 0.12 * math.sin(2 * math.pi * 2093 * t)
        else:
            phase = 2 * math.pi * (660 * t + (990 - 660) * t * t / (2 * end))
            value = math.sin(phase) + 0.12 * math.sin(2 * phase)
        values.append(value * envelope)
    scale = PEAK * 32767 / max(abs(value) for value in values)
    return [round(value * scale) for value in values]


def measure(path, expected_count):
    with wave.open(str(path), "rb") as audio:
        assert audio.getparams() == (1, 2, RATE, expected_count, "NONE", "not compressed")
        pcm = audio.readframes(audio.getnframes())
    values = struct.unpack(f"<{expected_count}h", pcm)
    peak = max(abs(value) for value in values) / 32768
    rms = math.sqrt(sum(value * value for value in values) / expected_count) / 32768
    dc = sum(values) / expected_count / 32768
    clipped = sum(value in (-32768, 32767) for value in values)
    start_step = values[1] - values[0]
    end_step = values[-1] - values[-2]
    max_step = max(abs(b - a) for a, b in zip(values, values[1:]))
    assert 0 < expected_count / RATE <= 1.2
    assert 0.01 <= rms <= peak <= 0.181
    assert abs(dc) < 0.0005 and clipped == 0
    assert values[0] == values[-1] == start_step == end_step == 0
    assert max_step / 32768 < 0.09
    return {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "bytes": path.stat().st_size,
        "sample_rate_hz": RATE,
        "channels": 1,
        "bits_per_sample": 16,
        "sample_count": expected_count,
        "duration_seconds": expected_count / RATE,
        "peak_normalized": round(peak, 9),
        "peak_dbfs": round(20 * math.log10(peak), 6),
        "rms_normalized": round(rms, 9),
        "dc_offset_normalized": round(dc, 9),
        "clipped_samples": clipped,
        "first_sample": values[0],
        "last_sample": values[-1],
        "start_step": start_step,
        "end_step": end_step,
        "max_adjacent_step_normalized": round(max_step / 32768, 9),
    }


def generate(output):
    output.mkdir(parents=True, exist_ok=True)
    filenames = [f"ovrcr-{tone[0]}-v1.wav" for tone in TONES] + ["manifest.json"]
    if any((output / name).exists() for name in filenames):
        raise SystemExit("Refusing to overwrite an existing generated asset or manifest.")
    manifest = {
        "schema_version": 1,
        "generator": "generate.py",
        "license": "MIT",
        "provenance": "Mathematical synthesis only; no sampled or extracted audio input.",
        "encoding": "WAV, little-endian signed 16-bit linear PCM, mono",
        "validation_limits": {
            "duration_seconds_max": 1.2,
            "peak_normalized_max": 0.181,
            "rms_normalized_min": 0.01,
            "dc_offset_normalized_abs_max": 0.0005,
            "clipped_samples_max": 0,
            "boundary_sample_and_step_abs_max": 0,
            "max_adjacent_step_normalized_max": 0.09,
        },
        "tones": [],
    }
    for tone in TONES:
        tone_id, label, _, _, _, _, description = tone
        path = output / f"ovrcr-{tone_id}-v1.wav"
        values = samples(tone)
        with wave.open(str(path), "wb") as audio:
            audio.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
            audio.writeframes(struct.pack(f"<{len(values)}h", *values))
        manifest["tones"].append({
            "id": tone_id, "label": label, "file": path.name,
            "description": description, **measure(path, len(values)),
        })
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python3 generate.py NEW_OUTPUT_DIRECTORY")
    generate(Path(sys.argv[1]))
