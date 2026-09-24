# EasyEDA to KiCad Converter

[![GitHub Release](https://img.shields.io/github/v/release/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI?sort=semver)](https://github.com/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI/releases/latest)

A Windows app that turns any part from [LCSC](https://www.lcsc.com/) or [EasyEDA](https://easyeda.com/) into a KiCad library: the schematic symbol, the PCB footprint, and the 3D model. Type the LCSC part number, press Convert, and add the library to KiCad. It is handy for boards assembled by [JLCPCB](https://jlcpcb.com/), which stocks the same parts.

It is one `.exe` with nothing to install. No Python, no command line.

## Download

1. Open the [latest release](https://github.com/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI/releases/latest).
2. Download `EasyEDA-to-KiCad-Converter.exe`.
3. Double-click it. If Windows SmartScreen says it protected your PC, choose **More info**, then **Run anyway**. The app is not code-signed.

It runs on Windows 10 and 11. It uses Microsoft Edge WebView2, which Windows 11 already has. On Windows 10 without it, install the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) first.

## Convert a Part

1. **LCSC part number:** Type the number from the part's LCSC or JLCPCB page, such as `C25804`. The app looks the part up and shows its name, manufacturer, and package.
2. **Output folder:** Choose where the library goes with **Browse**, or leave it empty to use `Documents\Kicad\easyeda2kicad`.
3. **Library layout:**
   - **Single Part Folder** puts each part in a folder of its own, named after the part, such as `0603WAF1002T5E_C25804`.
   - **Custom Library** writes into a library you name. Convert several parts under the same name and they collect in one library.
4. **Library name:** Filled in from the part. You can change it.
5. **Generate:** Choose any of **Symbol**, **Footprint**, and **3D model**. All three are on to start.
6. **Overwrite:** Off, the app refuses to replace a symbol, footprint, or 3D model the library already has, and changes nothing. On, it replaces them.
7. **Project relative:** With an output folder chosen, the footprint finds its 3D model through `${KIPRJMOD}`, KiCad's name for the project folder. Choose your KiCad project folder as the output folder and the project keeps working when it moves to another computer. Off, or with no folder chosen, the footprint stores the model's full path.
8. Press **Convert**. The app lists the files it saved, and **Open folder** shows them.

The window shows where the files will go before you convert.

## What You Get

For a library named `MyParts`:

| File | Contents |
| --- | --- |
| `MyParts.kicad_sym` | The symbol library. Each converted part is one symbol in it. |
| `MyParts.pretty/` | The footprint library, one `.kicad_mod` file per footprint. |
| `MyParts.3dshapes/` | The 3D models, as `.wrl` for KiCad's viewer and `.step` for mechanical CAD. |

## Add the Library to KiCad

1. In KiCad, open **Preferences > Manage Symbol Libraries** and add `MyParts.kicad_sym`.
2. Open **Preferences > Manage Footprint Libraries** and add the `MyParts.pretty` folder. Give it the nickname `MyParts`, the library name, since each symbol refers to its footprint as `MyParts:<footprint>`.

Add them on the **Project Specific Libraries** tab when the library lives in the project folder.

## When Something Goes Wrong

The app says what failed and what to do next, with the underlying error below it.

- **EasyEDA turned the request away:** EasyEDA limits how many requests one computer sends in a short time. Wait a minute, then convert again.
- **EasyEDA has no part:** Check the number on the part's LCSC page. It starts with `C`.
- **The library already has it:** Turn on **Overwrite** to replace the existing symbol, footprint, or 3D model.
- **No 3D model:** Some parts have none on EasyEDA. The symbol and footprint are still saved.

## How It Works

The app talks to the EasyEDA API directly and writes the KiCad files itself. The conversion is a line-by-line port of [easyeda2kicad.py](https://github.com/uPesy/easyeda2kicad.py) 1.0.1 by uPesy into Rust, and for the same part it writes the same symbol, footprint, and 3D model text. Files are saved with Unix line endings, as KiCad itself saves them. The app is licensed under the AGPL-3.0, like easyeda2kicad.py. See [`Web/LICENSE`](Web/LICENSE).

## Repository Layout

| Folder | Contents |
| --- | --- |
| [`Web/`](Web) | The app: a [Tauri 2](https://tauri.app/) window with a React front end and the Rust converter in `Web/src-tauri`. |
| [`Python/`](Python) | The earlier Python version, which needs Python and the `easyeda2kicad` package. See [its README](Python/README.md). |

## Build From Source

Needs Node.js 22.18 or later, pnpm 10, and Rust with the MSVC toolchain.

```sh
cd Web
pnpm install
pnpm tauri dev     # a development window
pnpm tauri build   # the release exe
```

`pnpm tauri build` writes the portable exe to `Web/src-tauri/target/release/EasyEDA-to-KiCad-Converter.exe`. It builds no installer.

Checks:

```sh
pnpm typecheck && pnpm lint && pnpm test && pnpm format:check && pnpm check:emdash
cd src-tauri && cargo test && cargo clippy --all-targets
```

`cargo test --test wellformed live -- --ignored` also converts real parts from EasyEDA and checks every file it writes. `cargo test --test wellformed produced -- --ignored`, with `WELLFORMED_DIR` set to a folder, checks every symbol library and footprint under it. The `saved_parts` example in `Web/src-tauri/examples` saves parts to disk in easyeda2kicad's cache layout and converts them from there, which is how the output is compared with easyeda2kicad.py on identical input.
