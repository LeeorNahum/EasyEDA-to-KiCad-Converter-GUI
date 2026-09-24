# EasyEDA to KiCad Converter GUI (Python, legacy)

This folder holds the original Python version of the converter, kept for anyone who already runs it. New users should use the Windows app described in the [main README](../README.md), which needs no Python and no command-line tools.

The Python version is a Tkinter window around the [`easyeda2kicad`](https://github.com/uPesy/easyeda2kicad.py) command-line tool. It fetches a part from [EasyEDA](https://easyeda.com/) or [LCSC](https://www.lcsc.com/) and writes a KiCad symbol, footprint, and 3D model by running that tool for you.

## Requirements

- Python 3.9 or later
- The `easyeda2kicad` package, 1.0.0 or later:

```bash
python -m pip install --upgrade easyeda2kicad
```

Older versions such as `0.8.0` fail against the current EasyEDA API.

## Run It

From this folder:

```bash
python easyeda2kicad_gui.py
```

`build.bat` packages it as a single `easyeda2kicad_gui.exe` with PyInstaller. That exe still needs Python and `easyeda2kicad` installed on the computer, because it runs the `easyeda2kicad` command.

## Using the GUI

1. **LCSC Part #:** Enter the LCSC part number, for example `C5267399`. The GUI looks up the part name and suggests a library name.
2. **Output Folder (Optional):** Click **Browse** to choose a base folder. Left empty, the GUI uses `C:/Users/your_name/Documents/Kicad/easyeda2kicad/`.
3. **Destination Mode:**
   - **Single Part Folder:** Creates one folder per part, named after the detected part.
   - **Custom Library:** Uses one library name you choose, so several parts merge into it.
4. **Options:** **Full** generates the symbol, footprint, and 3D model. Clear it to pick **Symbol**, **Footprint**, or **3D Model** on their own.
5. **Advanced Options:**
   - **Overwrite:** Replaces files that already exist.
   - **Project Relative:** When an Output Folder is set, 3D model paths are stored relative to it as `${KIPRJMOD}`.
   - **KiCad v5:** Asks for the legacy KiCad 5 format. `easyeda2kicad` 1.0 removed this option, so a conversion with it checked fails.
   - **Debug:** Shows the tool's full output in the error popup when a conversion fails.
6. **Run:** Enabled once the inputs are valid. The bottom box shows the exact command it runs.
