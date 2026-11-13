
import tkinter as tk
import numpy as np
import sounddevice as sd
import math

# -------------------------
# AUDIO ENGINE
# -------------------------

SAMPLE_RATE = 44100
BUFFER_SIZE = 256
DEVICE = 5

frequency = 440.0
amplitude = 0.2
phase = 0.0

stream = None


def audio_callback(outdata, frames, time, status):
    global phase

    increment = 2 * np.pi * frequency / SAMPLE_RATE

    phases = phase + increment * np.arange(frames)
    samples = amplitude * np.sin(phases)

    outdata[:, 0] = samples

    phase = (phase + increment * frames) % (2 * np.pi)


def start_audio():
    global stream

    if stream is not None:
        return

    stream = sd.OutputStream(
        device=DEVICE,
        samplerate=SAMPLE_RATE,
        blocksize=BUFFER_SIZE,
        channels=1,
        dtype="float32",
        callback=audio_callback
    )

    stream.start()
    status_label.config(text="● PLAYING", fg="#55e69a")


def stop_audio():
    global stream

    if stream is not None:
        stream.stop()
        stream.close()
        stream = None

    status_label.config(text="● STOPPED", fg="#777777")


# -------------------------
# GUI
# -------------------------

root = tk.Tk()
root.title("NOMAD SYNTH // 001")
root.geometry("720x520")
root.configure(bg="#101310")
root.resizable(False, False)

def update_frequency(value):
    global frequency
    frequency = float(value)
    freq_label.config(text=f"{frequency:.0f} Hz")


def update_amplitude(value):
    global amplitude
    amplitude = float(value) / 100
    amp_label.config(text=f"{int(float(value))} %")


def draw_wave():
    canvas.delete("wave")

    width = 660
    height = 180
    center = height / 2

    canvas.create_line(
        0, center, width, center,
        fill="#39443b",
        tags="wave"
    )

    points = []

    for x in range(width):
        t = x / width * 0.02
        y = center - math.sin(2 * math.pi * frequency * t) * 65
        points.extend([x, y])

    canvas.create_line(
        points,
        fill="#55e69a",
        width=2,
        tags="wave"
    )

    root.after(40, draw_wave)


# -------------------------
# HEADER
# -------------------------

header = tk.Frame(root, bg="#101310")
header.pack(fill="x", padx=30, pady=20)

tk.Label(
    header,
    text="NOMAD SYNTH",
    font=("Arial", 20, "bold"),
    fg="#55e69a",
    bg="#101310"
).pack(side="left")

status_label = tk.Label(
    header,
    text="● STOPPED",
    font=("Arial", 10),
    fg="#777777",
    bg="#101310"
)
status_label.pack(side="right")


# -------------------------
# OSCILLOSCOPE
# -------------------------

tk.Label(
    root,
    text="OSCILLOSCOPE",
    font=("Arial", 10, "bold"),
    fg="#999999",
    bg="#101310"
).pack(anchor="w", padx=30, pady=(10, 5))

canvas = tk.Canvas(
    root,
    width=660,
    height=180,
    bg="#080b09",
    highlightthickness=1,
    highlightbackground="#29342b"
)
canvas.pack(padx=30)


# -------------------------
# CONTROLS
# -------------------------

controls = tk.Frame(root, bg="#101310")
controls.pack(fill="x", padx=30, pady=25)

# Frequency

freq_frame = tk.Frame(controls, bg="#101310")
freq_frame.pack(side="left", expand=True, fill="x")

tk.Label(
    freq_frame,
    text="FREQUENCY",
    fg="#999999",
    bg="#101310"
).pack()

freq_label = tk.Label(
    freq_frame,
    text="440 Hz",
    font=("Arial", 22, "bold"),
    fg="#55e69a",
    bg="#101310"
)
freq_label.pack()

tk.Scale(
    freq_frame,
    from_=50,
    to=2000,
    orient="horizontal",
    command=update_frequency,
    bg="#101310",
    fg="white",
    troughcolor="#29342b",
    highlightthickness=0,
    length=260
).pack(pady=10)


# Amplitude

amp_frame = tk.Frame(controls, bg="#101310")
amp_frame.pack(side="right", expand=True, fill="x")

tk.Label(
    amp_frame,
    text="VOLUME",
    fg="#999999",
    bg="#101310"
).pack()

amp_label = tk.Label(
    amp_frame,
    text="20 %",
    font=("Arial", 22, "bold"),
    fg="#55e69a",
    bg="#101310"
)
amp_label.pack()

tk.Scale(
    amp_frame,
    from_=0,
    to=100,
    orient="horizontal",
    command=update_amplitude,
    bg="#101310",
    fg="white",
    troughcolor="#29342b",
    highlightthickness=0,
    length=260
).pack(pady=10)


# -------------------------
# TRANSPORT
# -------------------------

buttons = tk.Frame(root, bg="#101310")
buttons.pack(pady=10)

tk.Button(
    buttons,
    text="▶ PLAY",
    command=start_audio,
    bg="#55e69a",
    fg="#101310",
    font=("Arial", 11, "bold"),
    width=15,
    relief="flat"
).pack(side="left", padx=10)

tk.Button(
    buttons,
    text="■ STOP",
    command=stop_audio,
    bg="#303830",
    fg="white",
    font=("Arial", 11, "bold"),
    width=15,
    relief="flat"
).pack(side="left", padx=10)


def close_app():
    stop_audio()
    root.destroy()


root.protocol("WM_DELETE_WINDOW", close_app)

draw_wave()
root.mainloop()

