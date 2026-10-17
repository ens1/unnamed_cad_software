# Release review recordings — October 17, 2026

**Recording stopped at the user's request.** The user deferred videos and asked
to focus on the test drive. The UI review is now closed; no replacement or further
recording was made.

This index retains the status of the earlier capture attempt and recorder check.
They are not edited or narrated tutorials. Capture was limited to the TCAD
window and had no audio. The task capture failed and produced no movie.

Root folder: `/Users/tyler/tcad/release-review-2026-10-17/videos/`.

| ID | File | Status | Observed content | Build / theme | Useful range |
| --- | --- | --- | --- | --- | --- |
| 00 | [Recorder check](/Users/tyler/tcad/release-review-2026-10-17/videos/00-recording-check.mov), [review copy](/Users/tyler/tcad/release-review-2026-10-17/videos/00-recording-check-review.mp4) | File existence confirmed; reviewer reported a clean 15-second window-only silent check | Recorder setup check, not a feature demonstration | `c6b7343`; Classic | Check only |
| 01 | Intended filename `20261017-122437-672085-01-theme-and-preferences.mov` | Failed; movie does not exist | Intended task sequence was Classic settings, library setup and Nocturne preview/Cancel. No video evidence survives in the intended movie. | `c6b7343`; Classic and Nocturne preview | None |

The failed capture's [metadata](/Users/tyler/tcad/release-review-2026-10-17/videos/20261017-122437-672085-01-theme-and-preferences.json)
records a start at 17:24:37 UTC and end at 17:33:35 UTC, state `failed`, capture
exit code `-2` and zero bytes. Read-only filesystem inspection confirmed the
intended movie is absent. This recording failure is separate from TCAD's UI
observations. Screenshots remain available in the [report](report.md).

## Evidence required for future documentation clips

- Capture stopped cleanly and the resulting file exists with a usable duration.
- Playback inspected for readable text, expected window framing and unwanted
  interruptions or private content.
- Actual feature sequence and useful time ranges logged; failures kept distinct
  from clean demonstration takes.
- Starting/ending project paths recorded when the clip authors model content.
- A successful result labeled only for operations actually observed and saved.

The earlier proposed chapters in [coverage.md](coverage.md#recording-plan) are
deferred, not active deliverables for this session.
