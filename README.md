# EasyEDA to KiCad Converter GUI

[![Latest Release](https://img.shields.io/github/v/release/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI?display_name=tag)](https://github.com/LeeorNahum/EasyEDA-to-KiCad-Converter-GUI/releases)
[![Python](https://img.shields.io/badge/python-3.9%2B-blue)](https://www.python.org/)

**EasyEDA to KiCad Converter GUI** is a user-friendly graphical interface built with Python's Tkinter library. It allows you to effortlessly convert electronic components from [EasyEDA](https://easyeda.com/) or [LCSC](https://www.lcsc.com/) into KiCad-compatible libraries—including symbols, footprints, and 3D models. This tool streamlines your PCB design workflow, especially when utilizing [JLCPCB SMT assembly services](https://jlcpcb.com/caa). The UI is designed around a simple single-part workflow by default, while still allowing custom library merges when you need them.

![Screenshot of the program GUI](https://github.com/user-attachments/assets/03a8327a-43cb-4671-a487-c00fb5cebaf0)

## Features

- **Graphical User Interface (GUI):** Intuitive interface for easy operation without command-line interactions.
- **Comprehensive Conversion:** Convert symbols, footprints, and 3D models in a single operation.
- **Single Part Folder Mode:** Auto-detect the part name and create one clean folder per imported LCSC part.
- **Custom Library Mode:** Merge multiple parts into one named library when you actually want that workflow.
- **Auto-Filled Library Names:** Suggests a safe library name from fetched part metadata.
- **Support for Multiple KiCad Versions:** Compatible with both KiCad v6.x and v5.x library formats.
- **Custom Output Paths:** Specify custom directories for output libraries and models.
- **Overwrite Control:** Option to overwrite existing library files if needed.
- **Project Relative Paths:** Ensure project portability by setting 3D model paths relative to the project directory.
- **Better Error Popups:** Shows real CLI output in an error popup when a conversion fails.
- **Debug Mode by Default:** Keeps failure details visible without cluttering the main UI.
- **Tooltips:** Hover over elements for additional information and guidance.

## Install Dependency

### **EasyEDA2KiCad Package:** Install or upgrade the `easyeda2kicad` package using `pip`

```bash
python -m pip install --upgrade easyeda2kicad
```

The GUI expects a current `easyeda2kicad` release. Older versions such as `0.8.0` can fail against the current EasyEDA API.

## Using the GUI

1. **LCSC Part #:**
   - Enter the LCSC Part Number (e.g., `C5267399`) in the entry field
   labeled "LCSC Part #".
   - The GUI automatically looks up the part name and suggests a library name.

2. **Output Folder (Optional):**
   - Click the "Browse" button to select a base folder where the libraries will be saved.
   - If left empty, the GUI uses the default folder: `C:/Users/your_name/Documents/Kicad/easyeda2kicad/`.

3. **Destination Mode:**
   - **Single Part Folder:** Recommended default. Creates one folder per imported part using the detected name.
   - **Custom Library:** Lets you choose a reusable library name when you want to merge parts.

4. **Library Name:**
   - In **Single Part Folder** mode, the name is auto-filled.
   - In **Custom Library** mode, you can edit it manually.

5. **Options:**
   - **Full:** Generates Symbol, Footprint, and 3D Model. Enabled by default.
   - **Symbol:** Generates only the schematic symbol.
   - **Footprint:** Generates only the PCB footprint.
   - **3D Model:** Generates only the 3D model.

6. **Advanced Options:**
   - **Overwrite:** Overwrite existing library files if they already exist.
   - **Project Relative:** Set 3D model paths relative to the project directory for portability. Requires an Output Folder.
   - **KiCad v5:** Convert the library to legacy format for KiCad version 5.x.
   - **Debug:** Enabled by default so conversion failures show the real CLI error in a popup.

7. **Run:**
   - The "Run" button becomes enabled when all required inputs are valid.
   - Click "Run" to execute the conversion.

8. **Destination Preview and Command Preview:**
   - The GUI shows where the generated files will be written.
   - The exact CLI command is shown at the bottom for verification and troubleshooting.
