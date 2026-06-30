import json
import subprocess
import sys

import cv2
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


def detect_face(image):
    cascade = cv2.CascadeClassifier(
        cv2.data.haarcascades + "haarcascade_frontalface_default.xml"
    )
    gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY)
    faces = cascade.detectMultiScale(gray, scaleFactor=1.1, minNeighbors=4)
    if len(faces) == 0:
        return None
    x, y, w, h = max(faces, key=lambda face: face[2] * face[3])
    return x + w // 2, y + h // 2


def main():
    if len(sys.argv) != 4:
        print("usage: face_crop.py <video> <start> <end>", file=sys.stderr)
        return 2

    video, start, end = sys.argv[1:]
    width, height = dims(video)
    mid = timestamp((seconds(start) + seconds(end)) / 2)
    image = frame(video, mid, width, height)

    face = detect_face(image)
    if face is None:
        return 3
    face_cx, face_cy = face

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
