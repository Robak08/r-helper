
# Startup & Control tab performance (v0.4.8)
## Features

- **Faster startup** — Device presence is checked lightly in the background; full HID connect and state load happen once on the UI thread.
- **Loading state** — Control panel shows a spinner until init finishes instead of rendering all sections with “Reading…” placeholders.
- **Background device poller** — Fan RPM, brightness, AC power, and battery care sync on a background thread; UI applies snapshots without blocking.
- **Single specs query** — CPU, GPU, and RAM are read in one PowerShell call instead of three separate processes.
- **Smarter repaints** — 100 ms while initializing or dragging sliders; 250 ms when idle. Init status text no longer repaints every frame.

# Info tab & live device status (v0.4.7)
## Features

- **Control | Info tabs** — Switch between device controls and a read-only status view.
- **Laptop info card** — Shows model (with year), CPU, RAM, GPU, SKU, and USB PID.
- **Battery card** — Windows charge level, charging state, time remaining, and charge limit.
- **Connected peripherals** — Lists Razer mice/keyboards with battery % and status (laptop excluded).


# Saved settings & config persistence (v0.4.6)
## Features

- **Saved Settings** — Save full device state (performance, fan, lighting, battery) separately for AC and battery power.
- **Auto-switch** — Apply the matching saved profile when AC is plugged or unplugged.
- **Config file** — Settings stored in `%APPDATA%/r-helper/config.json` and restored on launch.
- **Auto fan cap** — Max RPM limit in auto fan mode persists across restarts.
- **Footer preferences** — Debug, minimize-to-tray, and run-at-startup checkboxes are remembered.

