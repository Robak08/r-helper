> [!WARNING]  
> Note from Fatalution: I returned my Blade 16.    

# R-Helper

A Windows application to control Razer Blade settings w/o Synapse.

<img width="332" height="388" alt="image" src="https://github.com/user-attachments/assets/3a4630d8-d79a-4e6b-b6a6-df4f1f52bdb9" />

## Features

- Performance modes: Battery, Silent, Balanced, Performance, Hyperboost, Custom
- Custom mode: CPU/GPU Low/Medium/High/Boost adjustments with experimental Undervolt option (no idea what it does as it's a preset)
- Fan control: Auto/Manual, with current RPM display
- Keyboard backlight brightness control
- Logo lighting: Static, Breathing, Off
- Battery care: Toggle charging threshold (80%)
- **Razer Laptop Cooling Pad** (USB `1532:0F43`): fan on/off with RPM control (500–3200), underglow lighting (Off / Static / Breathing + brightness)

> **Cooling pad note:** Close Razer Synapse or set the pad to Manual there before using r-helper — both apps control the pad over USB HID and will conflict otherwise. The pad has no real RPM sensor; displayed RPM is the last commanded value.


## Installation

1. Download the latest release
2. Run `rhelper.exe`

## Building

Release build with versioned packaging (recommended):

```powershell
.\scripts\package-release.ps1
```

Or use the cargo wrapper, which copies to `dist/` after `cargo build --release` finishes:

```powershell
.\scripts\cargo.ps1 build --release
```

Plain `cargo build --release` also copies to `dist/rhelper-<version>.exe` (e.g. `dist/rhelper-0_8_6.exe`) when the project is recompiled. Set `RHELPER_SKIP_RELEASE_PACKAGING=1` to disable automatic packaging.

To refresh `dist/` without recompiling:

```powershell
.\scripts\copy-release.ps1
```

## Architecture

Core device control via locally vendored `librazer` (derived from razer-ctl)


## License

MIT. Includes MIT-licensed portions derived from razer-ctl (see NOTICE and THIRD_PARTY_LICENSES.md).

## Support

If you really want to express gratitude: [PayPal Donation](https://www.paypal.com/paypalme/fatalutionDE)
