import json
import subprocess
import sys

import mediapipe as mp
import numpy as np


def run(cmd):
    return subprocess.check_output(cmd, stderr=subprocess.DEVNULL)


def dims(video):
    raw = run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "json",
            video,
        ]
    )
    stream = json.loads(raw)["streams"][0]
    return int(stream["width"]), int(stream["height"])


def frame(video, timestamp, width, height):
    raw = run(
        [
            "ffmpeg",
            "-ss",
            timestamp,
            "-i",
            video,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ]
    )
    return np.frombuffer(raw, dtype=np.uint8).reshape((height, width, 3))


def seconds(ts):
    h, m, s = ts.split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


def timestamp(seconds):
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds % 60
    return f"{h:02}:{m:02}:{s:06.3f}"


def clamp(v, low, high):
    return max(low, min(v, high))


def main():
    if len(sys.argv) != 4:
        print("usage: face_crop.py <video> <start> <end>", file=sys.stderr)
        return 2

    video, start, end = sys.argv[1:]
    width, height = dims(video)
    mid = timestamp((seconds(start) + seconds(end)) / 2)
    image = frame(video, mid, width, height)

    detector = mp.solutions.face_detection.FaceDetection(
        model_selection=1,
        min_detection_confidence=0.45,
    )
    result = detector.process(image)
    if not result.detections:
        return 3

    box = result.detections[0].location_data.relative_bounding_box
    face_cx = int((box.xmin + box.width / 2) * width)
    face_cy = int((box.ymin + box.height / 2) * height)

    crop_w = min(width, int(height * 9 / 16))
    crop_h = min(height, int(crop_w * 16 / 9))
    if crop_h > height:
        crop_h = height
        crop_w = int(crop_h * 9 / 16)

    x = clamp(face_cx - crop_w // 2, 0, width - crop_w)
    y = clamp(face_cy - crop_h // 2, 0, height - crop_h)
    print(f"crop={crop_w}:{crop_h}:{x}:{y}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
