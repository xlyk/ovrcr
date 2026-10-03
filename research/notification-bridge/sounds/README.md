# Original notification tone candidates

Exactly three custom assets for the proposed Bridge sound selector. Stable IDs are `tap`, `chime` and `rise`; versioned resource filenames are listed below. System default uses no custom asset. These research files are not an installed Bridge bundle or a shipped selector.

| ID / label | Resource | Duration | Samples | RMS, full scale | DC offset, full scale |
| --- | --- | ---: | ---: | ---: | ---: |
| `tap` / Tap | `ovrcr-tap-v1.wav` | 0.22 s | 10,560 | 0.037517918 | -0.000000786 |
| `chime` / Chime | `ovrcr-chime-v1.wav` | 0.84 s | 40,320 | 0.041941967 | 0.000000042 |
| `rise` / Rise | `ovrcr-rise-v1.wav` | 0.64 s | 30,720 | 0.084137228 | -0.000000297 |

All files are mono, 48,000 Hz, little-endian signed 16-bit linear PCM WAV. Their quantized peak is 0.179992676 full scale (-14.894903 dBFS). Every file has zero clipped samples, zero first/last samples and zero first/last adjacent sample steps. The [manifest](manifest.json) contains SHA-256 hashes, byte counts and full numeric measurements/limits, including the maximum adjacent sample step. Full-scale ratios use 32768 as the denominator.

## Synthesis and provenance

[generate.py](generate.py) produces the bytes solely with Python's standard-library `math`, `wave` and `struct` facilities, plus filesystem/manifest utilities. It takes no audio input and includes no samples, extracted system sound, third-party sound or generation dependency.

- **Tap:** 780 Hz sine plus a 0.20-weight 1560 Hz harmonic, with a 6 ms attack, 40 ms release and rapid exponential decay.
- **Chime:** 1046.5, 1569.75 and 2093 Hz sine partials weighted 1, 0.32 and 0.12, with a 10 ms attack, 120 ms release and exponential decay.
- **Rise:** a continuous linear-frequency glide from 660 to 990 Hz plus a 0.12-weight second harmonic, with a 12 ms attack, 80 ms release and slower exponential decay.

Each envelope uses squared-sine attack/release ramps and exponential decay, then scales the peak before signed 16-bit quantization. Descriptions state the construction; no listening assessment was performed.

The generator and assets are contributed under the repository's **MIT** license. The existing project notice is repeated in [LICENSE](LICENSE); retain it when packaging or redistributing these files. This provenance describes the generation method and intended redistribution license without asserting copyrightability or exclusive ownership of generated output.

## Reproduce and validate

Generate into two fresh directories; the script refuses to overwrite generated filenames:

```sh
python3 research/notification-bridge/sounds/generate.py /tmp/ovrcr-tones-a
python3 research/notification-bridge/sounds/generate.py /tmp/ovrcr-tones-b
diff -r /tmp/ovrcr-tones-a /tmp/ovrcr-tones-b
```

Generation reads back each written WAV and checks rate, channels, bit depth, frame count, duration, peak, RMS, DC, clipping and endpoint continuity. Two independent runs on CPython 3.13.7 / macOS 26.5.2 arm64 produced byte-identical WAVs **and manifests**; checked-in assets match those outputs. Floating-point transcendental functions depend on the Python/platform math implementation, so this result establishes reproducibility in that environment rather than a cross-platform byte guarantee.

Read-only inspection also succeeded for all three files:

```sh
afinfo PATH_TO_WAV
ffprobe -v error -show_entries stream=codec_name,sample_rate,channels,bits_per_sample,duration,duration_ts,time_base -show_entries format=format_name,duration,size -of json PATH_TO_WAV
```

`ffprobe` identified `pcm_s16le`, mono 48 kHz, 16 bits, and the exact durations/frame counts above. `afinfo` exited successfully and independently identified the PCM WAV encoding. No generation or decoder attempt failed. Actual commands, outputs, environment, hashes and byte comparisons are retained in `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-original-tones-20261003-ckwr2b_d/` for review; this temporary directory is not a durable public attachment.

Apple's current [UNNotificationSound documentation](https://developer.apple.com/documentation/usernotifications/unnotificationsound), retrieved 2026-10-03, supports Linear PCM in WAV and requires custom sounds shorter than 30 seconds. These files meet those numerical format/duration conditions. No playback, preview, notification delivery, native sound probe, signing or installation was performed. Native delivery and subjective sound selection remain later acceptance work.
