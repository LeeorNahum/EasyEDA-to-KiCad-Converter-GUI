# EasyEDA to KiCad Converter

[![GitHub Release](https://img.shields.io/github/v/release/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI?sort=semver)](https://github.com/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI/releases/latest)

A Windows app that turns any part from [LCSC](https://www.lcsc.com/) or [EasyEDA](https://easyeda.com/) into a KiCad library: the schematic symbol, the PCB footprint, and the 3D model. Type the LCSC part number and press Convert. Convert into a KiCad project and the part is ready to place: its libraries are added to the project, and its footprint and 3D model are linked, with nothing to set up in KiCad. It is handy for boards assembled by [JLCPCB](https://jlcpcb.com/), which stocks the same parts.

It is one `.exe` with nothing to install. It needs no Python. Double-click it for the window, or run it from a terminal with part numbers to convert without one.

## Download

1. Open the [latest release](https://github.com/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI/releases/latest).
2. Download `EasyEDA-to-KiCad-Converter.exe`.
3. Double-click it. If Windows SmartScreen says it protected your PC, choose **More info**, then **Run anyway**. The app is not code-signed.

It runs on Windows 10 and 11. It uses Microsoft Edge WebView2, which Windows 11 already has. On Windows 10 without it, install the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) first.

## Convert a Part

1. **LCSC part number:** Type the number from the part's LCSC or JLCPCB page, such as `C25804`. The app looks the part up and shows its name, manufacturer, and package.
2. **Output folder:** Choose where the library goes with **Browse**, or leave it empty to use `Documents\Kicad\easyeda2kicad`. Choose your KiCad project folder, or a folder inside it such as `lib`, to add the part to that project. The window then shows the project it links to. The app remembers the folder, the layout, and the Generate choices for next time.
3. **Library layout:**
   - **Single Part Folder** puts each part in a folder of its own, named after the part, such as `0603WAF1002T5E_C25804`.
   - **Custom Library** writes into a library you name. Convert several parts under the same name and they collect in one library.
4. **Library name:** Filled in from the part. You can change it.
5. **Generate:** Choose any of **Symbol**, **Footprint**, and **3D model**. All three are on until you change them.
6. **Overwrite:** Off, the app refuses to replace a symbol, footprint, or 3D model the library already has, and changes nothing. On, it replaces them.
7. **Project relative:** Shown only for a folder outside a KiCad project. With an output folder chosen, the footprint finds its 3D model through `${KIPRJMOD}`, KiCad's name for the project folder, taking the output folder as the project folder. Off, or with no folder chosen, the footprint stores the model's full path. Inside a project, paths are always relative to it.
8. Press **Convert**. The app says how many files it saved and, in a project, the name to add the symbol by. **Open folder** shows the files.

The window shows where the files will go before you convert. Outside a KiCad project it also shows where the footprint will look for its 3D model.

## What You Get

For a library named `MyParts`:

| File | Contents |
| --- | --- |
| `MyParts.kicad_sym` | The symbol library. Each converted part is one symbol in it. |
| `MyParts.pretty/` | The footprint library, one `.kicad_mod` file per footprint. |
| `MyParts.3dshapes/` | The 3D models, as `.wrl` for KiCad's viewer and `.step` for mechanical CAD. |

## In a KiCad Project

A folder is in a KiCad project when it, or a folder above it, holds a `.kicad_pro` file. The window says which project it found, or that the folder is in none. Converting into a project also does these, for whichever of the symbol, footprint, and 3D model you generate:

- Adds the symbol library and the footprint library to the project's own library tables, `sym-lib-table` and `fp-lib-table` beside the `.kicad_pro`, creating them when the project has none. Each is listed under the library name, with a path through `${KIPRJMOD}`. These are the libraries on the **Project Specific Libraries** tab of KiCad's Manage Libraries.
- Points each footprint at its `.wrl` 3D model through `${KIPRJMOD}`.
- Sets each symbol's Footprint field to its footprint, so **Update PCB from Schematic** places the right one.

In the schematic, press **A** and find the symbol by the name the app shows, like `INA226AIDGSR_C49851:INA226AIDGSR`. In the Single Part Folder layout the library name ends in the LCSC number, which tells it apart from KiCad's own symbol for the same part.

A library the project already lists, under any name, is left as it is, and the symbol uses that name. Nothing else in the tables changes, and converting a part again adds no second entry. If KiCad has the project open while you convert, close the project and open it again to load the new libraries.

A part converted into a project by an earlier version is not listed, and its footprint looks for the 3D model one folder off. Convert it again with **Overwrite** on to fix both.

The Single Part Folder layout suits a team sharing a project through Git: each part is its own files and its own table entry, so two people adding parts rarely change the same lines.

## Outside a KiCad Project

For a library outside a project, add it to KiCad yourself:

1. In KiCad, open **Preferences > Manage Symbol Libraries** and add `MyParts.kicad_sym`.
2. Open **Preferences > Manage Footprint Libraries** and add the `MyParts.pretty` folder. Give it the nickname `MyParts`, the library name, since each symbol refers to its footprint as `MyParts:<footprint>`.

## When Something Goes Wrong

The app says what failed and what to do next, with the underlying error below it.

- **EasyEDA turned the request away:** EasyEDA limits how many requests one computer sends in a short time. Wait a minute, then convert again.
- **EasyEDA has no part:** Check the number on the part's LCSC page. It starts with `C`.
- **The part is already there:** In a project that lists the part's libraries, the app says the symbol is ready to place and gives its name. Elsewhere it names what the library already has. Either way nothing is changed, and **Overwrite** replaces it.
- **The project already has a library with that name:** Another library in the project uses the name. Change the library name, or remove the other library in KiCad's Manage Libraries.
- **KiCad cannot read the project's library table:** The app changes nothing when a table is one KiCad itself could not open. Repair the file or restore an earlier copy of it, then convert again.
- **No 3D model:** Some parts have none on EasyEDA. The rest is still saved.

## Command Line

The same exe converts parts from a terminal or a script, the same way as the window. Give it part numbers and it runs there instead of opening the window. From a KiCad project folder that has a `lib` folder, with the exe on your `PATH`:

```sh
EasyEDA-to-KiCad-Converter C49851 C25804 --output lib
```

Without it on your `PATH`, name the exe by its path. In PowerShell that takes `&` and quotes:

```powershell
& "C:\Users\you\Desktop\EasyEDA-to-KiCad-Converter.exe" C49851 C25804 --output lib
```

That converts both parts into `lib`, each in a folder of its own, and adds them to the project. With no `--output`, the parts go in the current folder.

| Option | Effect |
| --- | --- |
| `-o, --output <folder>` | The folder the libraries go in. |
| `-l, --library <name>` | Puts every part in one library with this name, like Custom Library. |
| `--only <items>` | Writes only some of `symbol`, `footprint`, and `model`, like `--only symbol,footprint`. |
| `--overwrite` | Replaces a symbol, footprint, or 3D model the library already has. |
| `--full-model-paths` | Outside a KiCad project, stores each 3D model's full path, like turning off Project relative. |
| `--json` | Prints the result as JSON. |

It exits with 0 when every part converted, 1 when any did not, and 2 when the command itself is wrong. `EasyEDA-to-KiCad-Converter --help` lists the options.

On Windows 11 24H2 and later, double-clicking the exe opens only the window. On earlier Windows a console window appears for a moment first and closes by itself.

## How It Works

The app talks to the EasyEDA API directly and writes the KiCad files itself. The conversion is a function-by-function port of [easyeda2kicad.py](https://github.com/uPesy/easyeda2kicad.py) 1.0.1 by uPesy into Rust, and for the same part it writes the same symbol, footprint, and 3D model text, with a few corrections:

- Holes EasyEDA marks as unplated stay unplated.
- Custom-shaped pads keep their exact size, without an extra 0.1 mm outline.
- A symbol's Footprint field names the footprint as it is saved, rather than by the symbol's own package name, which can differ.
- Names and text with spaces or quotes are quoted, so KiCad reads them back as written.
- A part whose data has a missing or broken coordinate is refused instead of written.
- Converting into an existing library never leaves it half-written or with a symbol twice.

Files use Unix line endings, as KiCad itself writes them. The app is licensed under the AGPL-3.0, like easyeda2kicad.py. See [`Web/LICENSE`](Web/LICENSE).

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
