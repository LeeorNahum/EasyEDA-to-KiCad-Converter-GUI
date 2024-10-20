# EasyEDA to KiCad Converter GUI

**EasyEDA to KiCad Converter GUI** is a user-friendly graphical interface built with Python's Tkinter library. It allows you to effortlessly convert electronic components from [EasyEDA](https://easyeda.com/) or [LCSC](https://www.lcsc.com/) into KiCad-compatible libraries, including symbols, footprints, and 3D models. This tool streamlines your PCB design workflow, especially when utilizing [JLCPCB SMT assembly services](https://jlcpcb.com/caa). It supports library formats for both KiCad v6.x and KiCad v5.x.

![Screenshot of the program GUI](https://github.com/user-attachments/assets/03a8327a-43cb-4671-a487-c00fb5cebaf0)

## Features

- **Graphical User Interface (GUI):** Intuitive interface for easy operation without command-line interactions.
- **Comprehensive Conversion:** Convert symbols, footprints, and 3D models in a single operation.
- **Support for Multiple KiCad Versions:** Compatible with both KiCad v6.x and v5.x library formats.
- **Custom Output Paths:** Specify custom directories for output libraries and models.
- **Overwrite Control:** Option to overwrite existing library files if needed.
- **Project Relative Paths:** Ensure project portability by setting 3D model paths relative to the project directory.
- **Debug Mode:** Enable detailed error messages for troubleshooting.
- **Tooltips:** Hover over elements for additional information and guidance.

## Install Dependency

### **EasyEDA2KiCad Package:** Install the `easyeda2kicad` package using `pip`

```bash
pip install easyeda2kicad
```

## Using the GUI

1. **LCSC Part #:**
   - Enter the LCSC Part Number (e.g., `C5267399`) in the entry field labeled "LCSC Part #".

2. **Output Folder (Optional):**
   - Click the "Browse" button to select a custom output directory where the libraries will be saved.
   - If not specified, the tool uses the default folder: `C:/Users/your_name/Documents/Kicad/easyeda2kicad/`.

3. **Library Name:**
   - Enter the desired name for the library (e.g., `MyLib`).
   - This field becomes enabled when an Output Folder is specified.

4. **Create Folder:**
   - Check this option to create a new folder named after the library within the selected Output Folder.

5. **Options:**
   - **Full:** Generates Symbol, Footprint, and 3D Model.
   - **Symbol:** Generates only the schematic symbol.
   - **Footprint:** Generates only the PCB footprint.
   - **3D Model:** Generates only the 3D model.

6. **Advanced Options:**
   - **Overwrite:** Overwrite existing library files if they already exist.
   - **Project Relative:** Set 3D model paths relative to the project directory for portability. Requires an Output Folder.
   - **KiCad v5:** Convert the library to legacy format for KiCad version 5.x.
   - **Debug:** Enable debug mode to display detailed error messages.

7. **Run:**
   - The "Run" button becomes enabled when all required inputs are valid.
   - Click "Run" to execute the conversion. The command being executed is displayed at the bottom of the window.

8. **Command Display:**
   - View the exact command that will be executed, useful for debugging or verification.
