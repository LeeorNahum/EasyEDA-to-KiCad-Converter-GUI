import tkinter as tk
from tkinter import filedialog, messagebox
import tkinter.font as tkFont
import subprocess
from pathlib import Path
import os
import re
import sys  # Ensure sys is imported
import threading
from importlib import metadata

MINIMUM_EASYEDA2KICAD_VERSION = (1, 0, 0)
MINIMUM_EASYEDA2KICAD_VERSION_STR = ".".join(
    str(part) for part in MINIMUM_EASYEDA2KICAD_VERSION
)
LOOKUP_DEBOUNCE_MS = 500

# Define resource_path
def resource_path(relative_path):
    """ Get absolute path to resource, works for dev and for PyInstaller """
    try:
        # PyInstaller creates a temp folder and stores path in _MEIPASS
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")
    return os.path.join(base_path, relative_path)

# Tooltip class for adding hover explanations
class ToolTip:
    def __init__(self, widget, text):
        self.widget = widget
        self.text = text
        self.tip_window = None
        self.id = None
        self.widget.bind("<Enter>", self.schedule)
        self.widget.bind("<Leave>", self.hide_tip)

    def schedule(self, event=None):
        self.unschedule()
        self.id = self.widget.after(500, self.show_tip)

    def unschedule(self):
        id_ = self.id
        self.id = None
        if id_:
            self.widget.after_cancel(id_)

    def show_tip(self, event=None):
        if self.tip_window or not self.text:
            return
        x, y, cx, cy = (0, 0, 0, 0)
        try:
            x, y, cx, cy = self.widget.bbox("insert")
        except:
            pass
        x += self.widget.winfo_rootx() + 25
        y += self.widget.winfo_rooty() + 20
        self.tip_window = tw = tk.Toplevel(self.widget)
        tw.wm_overrideredirect(True)
        tw.wm_geometry(f"+{x}+{y}")
        label = tk.Label(
            tw,
            text=self.text,
            justify=tk.LEFT,
            background="#ffffe0",
            relief=tk.SOLID,
            borderwidth=1,
            font=("tahoma", "8", "normal"),
            wraplength=300,
        )
        label.pack(ipadx=1)

    def hide_tip(self, event=None):
        self.unschedule()
        tw = self.tip_window
        self.tip_window = None
        if tw:
            tw.destroy()

# Enhanced PlaceholderEntry class
class PlaceholderEntry(tk.Entry):
    def __init__(self, master=None, placeholder="PLACEHOLDER", color='grey', textvariable=None, state='normal', *args, **kwargs):
        self.placeholder = placeholder
        self.placeholder_color = color
        self.default_fg_color = kwargs.get('fg', 'black')
        self.placeholder_active = False

        self.textvariable = textvariable or tk.StringVar()
        super().__init__(master, textvariable=self.textvariable, state=state, *args, **kwargs)

        # Bind events
        self.bind("<FocusIn>", self.foc_in)
        self.bind("<FocusOut>", self.foc_out)

        # Add a trace on the textvariable
        self.textvariable.trace_add('write', self.check_placeholder)

        # Initially put placeholder if needed
        self.put_placeholder()

    def put_placeholder(self):
        if not self.textvariable.get() and not self.focus_get() == self:
            self.placeholder_active = True
            if self['state'] == 'disabled':
                # Set disabledforeground to placeholder color
                self.config(disabledforeground=self.placeholder_color)
            else:
                self['fg'] = self.placeholder_color
            self.delete(0, 'end')
            self.insert(0, self.placeholder)

    def clear_placeholder(self):
        if self.placeholder_active:
            self.delete(0, 'end')
            if self['state'] == 'disabled':
                self.config(disabledforeground=self.default_fg_color)
            else:
                self['fg'] = self.default_fg_color
            self.placeholder_active = False

    def check_placeholder(self, *args):
        if self.placeholder_active:
            if self.textvariable.get() != self.placeholder:
                self.placeholder_active = False
                if self['state'] == 'disabled':
                    self.config(disabledforeground=self.default_fg_color)
                else:
                    self['fg'] = self.default_fg_color
        else:
            if not self.textvariable.get() and not self.focus_get() == self and self['state'] != 'disabled':
                self.put_placeholder()

    def foc_in(self, *args):
        if self.placeholder_active:
            self.clear_placeholder()

    def foc_out(self, *args):
        if not self.textvariable.get() and self['state'] != 'disabled':
            self.put_placeholder()

    def get_real_text(self):
        if self.placeholder_active:
            return ''
        else:
            return self.textvariable.get()

    def set_state(self, state):
        if state == 'disabled' and self.focus_get() == self:
            # Remove focus from this entry if it's being disabled while focused
            self.master.focus_set()
        self.config(state=state)
        if state == 'disabled':
            if not self.textvariable.get():
                self.put_placeholder()
            else:
                # Ensure text color is default when disabled
                self.config(disabledforeground=self.default_fg_color)
        elif state == 'normal':
            if self.placeholder_active:
                self.clear_placeholder()
            if not self.textvariable.get() and not self.focus_get() == self:
                self.put_placeholder()


def parse_version(version_string):
    parts = []
    for part in version_string.split("."):
        digits = ""
        for char in part:
            if char.isdigit():
                digits += char
            else:
                break
        parts.append(int(digits) if digits else 0)

    while len(parts) < 3:
        parts.append(0)

    return tuple(parts[:3])


def get_cli_version():
    try:
        return metadata.version("easyeda2kicad")
    except metadata.PackageNotFoundError:
        return None


def get_default_output_folder():
    return Path.home() / "Documents" / "Kicad" / "easyeda2kicad"


def sanitize_library_name(name):
    sanitized = re.sub(r'[<>:"/\\|?*\x00-\x1f]+', "_", name.strip())
    sanitized = re.sub(r"\s+", "_", sanitized)
    sanitized = re.sub(r"_+", "_", sanitized).strip("._ ")
    return sanitized or "easyeda2kicad"


def get_suggested_library_name(lcsc_id, title="", manufacturer_part=""):
    preferred_name = manufacturer_part or title or lcsc_id or "easyeda2kicad"
    if lcsc_id and lcsc_id not in preferred_name:
        preferred_name = f"{preferred_name}_{lcsc_id}"
    return sanitize_library_name(preferred_name)


def get_auto_library_name():
    lcsc_id = lcsc_entry.get_real_text().strip().upper()
    if not lcsc_id:
        return ""
    if (
        part_metadata.get("lcsc_id") == lcsc_id
        and part_metadata.get("library_name")
    ):
        return part_metadata["library_name"]
    return get_suggested_library_name(lcsc_id)


def get_effective_library_name():
    if destination_mode_var.get() == "single_part":
        return get_auto_library_name()
    library_name = library_name_entry.get_real_text().strip()
    if not library_name:
        return ""
    return sanitize_library_name(library_name)


def get_base_output_path():
    output_path = output_entry.get_real_text().strip()
    if output_path:
        return Path(output_path)
    return get_default_output_folder()


def get_output_target():
    base_output_path = get_base_output_path()
    library_name = get_effective_library_name()
    if not library_name:
        return None

    if destination_mode_var.get() == "single_part":
        return base_output_path / library_name / library_name

    return base_output_path / library_name


def get_command_working_directory():
    output_path = output_entry.get_real_text().strip()
    if project_relative_var.get() and output_path:
        return str(Path(output_path))
    return None


def set_part_status(message, color="dim gray"):
    part_info_var.set(message)
    if "part_info_label" in globals():
        part_info_label.config(fg=color)


def set_library_name_value(value):
    global library_name_internal_update
    library_name_internal_update = True
    library_name_var.set(value)
    library_name_internal_update = False


def apply_auto_library_name(force=False):
    global last_auto_library_name, custom_library_name_dirty
    suggested_name = get_auto_library_name()
    current_value = library_name_entry.get_real_text().strip()

    if force or not current_value or not custom_library_name_dirty or current_value == last_auto_library_name:
        last_auto_library_name = suggested_name
        set_library_name_value(suggested_name)
        if destination_mode_var.get() == "single_part":
            custom_library_name_dirty = False


def refresh_library_name_controls():
    if destination_mode_var.get() == "single_part":
        library_name_label.config(text="Library Name (Suggested)")
        apply_auto_library_name(force=False)
    else:
        library_name_label.config(text="Library Name")
        if not library_name_entry.get_real_text().strip():
            apply_auto_library_name(force=True)
    library_name_entry.set_state('normal')


def update_destination_preview(*args):
    output_target = get_output_target()
    if output_target:
        preview = f"Destination: {output_target}"
        if project_relative_var.get() and output_entry.get_real_text().strip():
            preview += " | 3D path stored relative to ${KIPRJMOD}"
        destination_preview_var.set(preview)
    else:
        destination_preview_var.set("Destination: uses the CLI default library path.")


def build_part_metadata(cad_data, lcsc_id):
    data_str = cad_data.get("dataStr") or {}
    head = data_str.get("head") or {}
    component_parameters = head.get("c_para") or {}

    title = (
        cad_data.get("title")
        or component_parameters.get("Manufacturer Part")
        or component_parameters.get("name")
        or lcsc_id
    )
    manufacturer = component_parameters.get("Manufacturer") or ""
    package = component_parameters.get("package") or ""
    part_class = component_parameters.get("JLCPCB Part Class") or ""
    library_name = get_suggested_library_name(
        lcsc_id,
        title=title,
        manufacturer_part=component_parameters.get("Manufacturer Part") or "",
    )

    return {
        "found": True,
        "lcsc_id": lcsc_id,
        "title": title,
        "manufacturer": manufacturer,
        "package": package,
        "part_class": part_class,
        "library_name": library_name,
    }


def fetch_part_metadata_worker(lcsc_id, request_id):
    cli_version = get_cli_version()
    if cli_version is None or parse_version(cli_version) < MINIMUM_EASYEDA2KICAD_VERSION:
        result = {
            "found": False,
            "lcsc_id": lcsc_id,
            "message": "Install or upgrade easyeda2kicad to auto-detect part details.",
        }
    else:
        try:
            from easyeda2kicad.easyeda.easyeda_api import EasyedaApi

            cad_data = EasyedaApi(use_cache=True).get_cad_data_of_component(lcsc_id)
            if cad_data:
                result = build_part_metadata(cad_data, lcsc_id)
            else:
                result = {
                    "found": False,
                    "lcsc_id": lcsc_id,
                    "message": "Could not fetch part details right now. The conversion can still run.",
                }
        except Exception as exc:
            result = {
                "found": False,
                "lcsc_id": lcsc_id,
                "message": f"Part lookup failed: {exc}",
            }

    root.after(0, lambda: apply_part_metadata(request_id, result))


def apply_part_metadata(request_id, result):
    global part_metadata

    if request_id != part_lookup_request_id:
        return

    part_metadata = result
    if result.get("found"):
        details = [result["title"]]
        for extra in (result.get("manufacturer"), result.get("package"), result.get("part_class")):
            if extra:
                details.append(extra)
        set_part_status("Detected: " + " | ".join(details), "dark green")
        apply_auto_library_name(force=False)
    else:
        set_part_status(result.get("message", "Part details unavailable."), "dark goldenrod")
        if destination_mode_var.get() == "single_part":
            apply_auto_library_name(force=False)

    refresh_library_name_controls()
    update_destination_preview()
    validate_inputs()
    update_command_display()


def schedule_part_lookup(*args):
    global part_lookup_after_id

    lcsc_id = lcsc_entry.get_real_text().strip().upper()
    if part_lookup_after_id is not None:
        root.after_cancel(part_lookup_after_id)
        part_lookup_after_id = None

    if not lcsc_id:
        set_part_status("Type an LCSC part number to auto-detect the part name.", "dim gray")
        update_destination_preview()
        return

    if not re.fullmatch(r"C\d+", lcsc_id):
        set_part_status("Enter a full LCSC part number like C209903.", "dim gray")
        update_destination_preview()
        return

    set_part_status("Looking up part details...", "dim gray")
    part_lookup_after_id = root.after(
        LOOKUP_DEBOUNCE_MS,
        lambda captured_id=lcsc_id: start_part_lookup(captured_id),
    )


def start_part_lookup(lcsc_id):
    global part_lookup_after_id, part_lookup_request_id

    part_lookup_after_id = None
    part_lookup_request_id += 1
    request_id = part_lookup_request_id

    lookup_thread = threading.Thread(
        target=fetch_part_metadata_worker,
        args=(lcsc_id, request_id),
        daemon=True,
    )
    lookup_thread.start()


def on_library_name_change(*args):
    global custom_library_name_dirty

    if library_name_internal_update:
        return

    current_value = library_name_entry.get_real_text().strip()
    if not current_value:
        custom_library_name_dirty = False
        return

    custom_library_name_dirty = current_value != last_auto_library_name


def on_destination_mode_change(*args):
    refresh_library_name_controls()
    update_destination_preview()
    validate_inputs()
    update_command_display()


def build_command():
    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()

    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()
    overwrite = overwrite_var.get()
    v5 = v5_var.get()
    project_relative = project_relative_var.get()
    debug = debug_var.get()

    command = ["easyeda2kicad"]

    if lcsc_id:
        command.extend(["--lcsc_id", lcsc_id])

    if full:
        command.append("--full")
    else:
        if symbol:
            command.append("--symbol")
        if footprint:
            command.append("--footprint")
        if model3d:
            command.append("--3d")

    output_target = get_output_target()
    if output_target:
        command.extend(["--output", str(output_target)])

    if overwrite:
        command.append("--overwrite")
    if v5:
        command.append("--v5")
    if project_relative and output_path:
        command.append("--project-relative")
    if debug:
        command.append("--debug")

    return command


def format_command(command):
    return ' '.join(f'"{arg}"' if ' ' in arg else arg for arg in command)


def get_command_output(result):
    return "\n".join(
        part.strip()
        for part in (result.stdout, result.stderr)
        if part and part.strip()
    )

def run_command():
    # Collect options
    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()
    library_name = get_effective_library_name()

    # Get the state of the checkbuttons
    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()
    overwrite = overwrite_var.get()
    v5 = v5_var.get()
    project_relative = project_relative_var.get() and bool(output_path)
    debug = debug_var.get()

    # Validate LCSC Part #
    if not lcsc_id:
        messagebox.showerror("Error", "Please enter the LCSC Part #.")
        return

    if not full and not (symbol or footprint or model3d):
        messagebox.showerror("Error", "Please select at least one option: Symbol, Footprint, or 3D Model.")
        return

    if project_relative and not output_path:
        messagebox.showerror("Error", "Project-relative option requires an Output Folder.")
        return

    cli_version = get_cli_version()
    if cli_version is None:
        messagebox.showerror(
            "Error",
            "easyeda2kicad is not installed.\n\n"
            "Install it with:\npython -m pip install --upgrade easyeda2kicad",
        )
        return

    if parse_version(cli_version) < MINIMUM_EASYEDA2KICAD_VERSION:
        messagebox.showerror(
            "Error",
            f"easyeda2kicad {cli_version} is too old for this GUI.\n\n"
            f"Please upgrade to {MINIMUM_EASYEDA2KICAD_VERSION_STR} or later:\n"
            "python -m pip install --upgrade easyeda2kicad",
        )
        return

    if output_path and not Path(output_path).is_dir():
        messagebox.showerror("Error", "Output Folder must already exist.")
        return

    if not library_name:
        messagebox.showerror("Error", "A library name could not be determined.")
        return

    output_target = get_output_target()
    if output_target:
        output_target.parent.mkdir(parents=True, exist_ok=True)

    # Build the command
    command = build_command()

    # Format the command string with quotes if necessary
    command_str = format_command(command)

    # Display the command in the command display box
    command_display.config(state='normal')
    command_display.delete(1.0, tk.END)
    command_display.insert(tk.END, command_str)
    command_display.config(state='disabled')

    # Run the command
    try:
        print("Running command:", ' '.join(command))
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            shell=False,
            cwd=get_command_working_directory(),
        )
        if result.returncode != 0:
            command_output = get_command_output(result)
            if command_output:
                messagebox.showerror("Error", f"Conversion failed:\n\n{command_output}")
            else:
                messagebox.showerror(
                    "Error",
                    "An error occurred during the conversion. Please check your inputs and try again.",
                )
        else:
            # Show success message
            success_message = "Conversion completed successfully."
            if project_relative:
                success_message += "\nNote: 3D model paths are set relative to the project directory (${KIPRJMOD})."
            if output_target:
                success_message += f"\nSaved under: {output_target}"
            messagebox.showinfo("Success", success_message)
    except FileNotFoundError:
        messagebox.showerror("Error", "Error: 'easyeda2kicad' command not found. Ensure it is installed and added to your PATH.")
    except Exception as e:
        messagebox.showerror("Error", f"An unexpected error occurred:\n{e}")

def on_full_toggle():
    if full_var.get():
        # Disable the individual options
        symbol_check.config(state="disabled")
        footprint_check.config(state="disabled")
        model3d_check.config(state="disabled")
        # Uncheck the individual options
        symbol_var.set(False)
        footprint_var.set(False)
        model3d_var.set(False)
    else:
        # Enable the individual options
        symbol_check.config(state="normal")
        footprint_check.config(state="normal")
        model3d_check.config(state="normal")
    validate_inputs()
    update_command_display()

def browse_output():
    path = filedialog.askdirectory()
    if path:
        output_var.set(path)
        # Move cursor to the end of the text box
        output_entry.focus()
        output_entry.icursor(tk.END)
        validate_inputs()
        update_command_display()

def update_command_display(*args):
    command = build_command()

    # Format the command string with quotes if necessary
    command_str = format_command(command)

    # Update the command display box
    command_display.config(state='normal')
    command_display.delete(1.0, tk.END)
    command_display.insert(tk.END, command_str)
    command_display.config(state='disabled')

def validate_inputs(*args):
    errors = []

    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()
    library_name = get_effective_library_name()
    if not lcsc_id:
        errors.append("LCSC Part # is required.")

    cli_version = get_cli_version()
    if cli_version is None:
        errors.append("easyeda2kicad is not installed.")
    elif parse_version(cli_version) < MINIMUM_EASYEDA2KICAD_VERSION:
        errors.append(
            f"easyeda2kicad {cli_version} is too old. Upgrade to "
            f"{MINIMUM_EASYEDA2KICAD_VERSION_STR}+."
        )

    if output_path and not Path(output_path).is_dir():
        errors.append("Output Folder must already exist.")

    # Check options
    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()

    if not full and not (symbol or footprint or model3d):
        errors.append("Select Full or at least one of Symbol, Footprint, or 3D Model.")

    if not library_name:
        if destination_mode_var.get() == "single_part":
            errors.append("Waiting for a valid LCSC part number to derive the library name.")
        else:
            errors.append("Library Name is required in Custom Library mode.")

    if errors:
        run_button.config(state='disabled', bg='SystemButtonFace', fg='SystemButtonText', text='Run')
        run_button_tooltip.text = "Cannot run due to the following errors:\n- " + "\n- ".join(errors)
    else:
        run_button.config(state='normal', bg='green', fg='white', text='Run')
        run_button_tooltip.text = "Execute the conversion with the selected options."

    refresh_library_name_controls()

# Create the main window
root = tk.Tk()
root.title("EasyEDA to KiCad Converter GUI")
root.resizable(False, False)

# Set the window icon using resource_path
icon_path = resource_path('easyeda2kicad_gui.ico')  # Use resource_path to locate the icon
try:
    root.iconbitmap(icon_path)
except Exception as e:
    print(f"Warning: Unable to set icon. {e}")

# Variables
lcsc_id_var = tk.StringVar()
output_var = tk.StringVar()
library_name_var = tk.StringVar()
destination_mode_var = tk.StringVar(value="single_part")
part_info_var = tk.StringVar(value="Type an LCSC part number to auto-detect the part name.")
destination_preview_var = tk.StringVar()
symbol_var = tk.BooleanVar()
footprint_var = tk.BooleanVar()
model3d_var = tk.BooleanVar()
full_var = tk.BooleanVar(value=True)
overwrite_var = tk.BooleanVar()
v5_var = tk.BooleanVar()
project_relative_var = tk.BooleanVar(value=True)
debug_var = tk.BooleanVar(value=True)

part_metadata = {"found": False, "lcsc_id": "", "library_name": ""}
part_lookup_after_id = None
part_lookup_request_id = 0
library_name_internal_update = False
custom_library_name_dirty = False
last_auto_library_name = ""

# Create the main frame and configure grid columns
main_frame = tk.Frame(root)
main_frame.pack(pady=(5, 0), padx=5)

# Configure grid columns to expand
main_frame.grid_columnconfigure(0, weight=1)
main_frame.grid_columnconfigure(1, weight=1)

# Padding options
padding_options = {'padx': 5, 'pady': 3}

# LCSC Part #
lcsc_entry = PlaceholderEntry(main_frame, placeholder="LCSC Part #", textvariable=lcsc_id_var)
lcsc_entry.grid(row=0, column=0, columnspan=2, sticky="ew", **padding_options)
ToolTip(lcsc_entry, "Enter the LCSC Part Number of the component (e.g., C5267399).")

part_info_label = tk.Label(
    main_frame,
    textvariable=part_info_var,
    anchor="w",
    justify="left",
    fg="dim gray",
    wraplength=470,
)
part_info_label.grid(row=1, column=0, columnspan=2, sticky="ew", **padding_options)

# Output Folder and Browse Button
output_frame = tk.Frame(main_frame)
output_frame.grid(row=2, column=0, columnspan=2, sticky="ew", **padding_options)
output_frame.grid_columnconfigure(0, weight=1)

output_entry = PlaceholderEntry(output_frame, placeholder="Output Folder (Optional)", textvariable=output_var)
output_entry.grid(row=0, column=0, sticky="ew")
ToolTip(output_entry, "Select the base folder where the library will be saved. Leave it blank to use Documents/Kicad/easyeda2kicad.")

browse_button = tk.Button(output_frame, text="Browse", command=browse_output)
browse_button.grid(row=0, column=1, padx=(5,0))
ToolTip(browse_button, "Click to browse and select the output folder.")

# Destination mode
destination_frame = tk.LabelFrame(main_frame, text="Destination")
destination_frame.grid(row=3, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
destination_frame.grid_columnconfigure((0, 1), weight=1)

single_part_radio = tk.Radiobutton(
    destination_frame,
    text="Single Part Folder",
    variable=destination_mode_var,
    value="single_part",
    command=on_destination_mode_change,
)
single_part_radio.grid(row=0, column=0, padx=5, sticky="w")
ToolTip(single_part_radio, "Best default for one-off imports. Uses the detected part name and creates a dedicated folder for that part.")

custom_library_radio = tk.Radiobutton(
    destination_frame,
    text="Custom Library",
    variable=destination_mode_var,
    value="custom_library",
    command=on_destination_mode_change,
)
custom_library_radio.grid(row=0, column=1, padx=5, sticky="w")
ToolTip(custom_library_radio, "Use this when you want to merge parts into one library name instead of one folder per part.")

# Library Name
library_frame = tk.Frame(main_frame)
library_frame.grid(row=4, column=0, columnspan=2, sticky="ew", **padding_options)
library_frame.grid_columnconfigure(1, weight=1)

library_name_label = tk.Label(library_frame, text="Library Name (Auto)")
library_name_label.grid(row=0, column=0, padx=(0, 5), sticky="w")

library_name_entry = PlaceholderEntry(library_frame, placeholder="Library Name", textvariable=library_name_var, state='disabled')
library_name_entry.grid(row=0, column=1, sticky="ew")
ToolTip(library_name_entry, "Single Part Folder starts with an auto-suggested name, but you can still edit it. Custom Library mode also lets you choose the name manually.")

destination_preview_label = tk.Label(
    main_frame,
    textvariable=destination_preview_var,
    anchor="w",
    justify="left",
    fg="dim gray",
    wraplength=470,
)
destination_preview_label.grid(row=5, column=0, columnspan=2, sticky="ew", **padding_options)

# Options: Full, Symbol, Footprint, 3D Model
options_frame = tk.LabelFrame(main_frame, text="Options")
options_frame.grid(row=6, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
options_frame.grid_columnconfigure((0,1,2,3), weight=1)

full_check = tk.Checkbutton(options_frame, text="Full", variable=full_var, command=on_full_toggle)
full_check.grid(row=0, column=0, padx=5, sticky="w")
ToolTip(full_check, "Select to generate Symbol, Footprint, and 3D Model.")

symbol_check = tk.Checkbutton(options_frame, text="Symbol", variable=symbol_var, command=validate_inputs)
symbol_check.grid(row=0, column=1, padx=5, sticky="w")
ToolTip(symbol_check, "Generate the schematic symbol for the component.")

footprint_check = tk.Checkbutton(options_frame, text="Footprint", variable=footprint_var, command=validate_inputs)
footprint_check.grid(row=0, column=2, padx=5, sticky="w")
ToolTip(footprint_check, "Generate the PCB footprint for the component.")

model3d_check = tk.Checkbutton(options_frame, text="3D Model", variable=model3d_var, command=validate_inputs)
model3d_check.grid(row=0, column=3, padx=5, sticky="w")
ToolTip(model3d_check, "Generate the 3D model for the component.")

# Options: Overwrite, Project Relative, KiCad v5, Debug
options_frame2 = tk.LabelFrame(main_frame, text="Advanced Options")
options_frame2.grid(row=7, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
options_frame2.grid_columnconfigure((0,1,2,3), weight=1)

overwrite_check = tk.Checkbutton(options_frame2, text="Overwrite", variable=overwrite_var, command=validate_inputs)
overwrite_check.grid(row=0, column=0, padx=5, sticky="w")
ToolTip(overwrite_check, "Overwrite existing library files if they already exist.")

project_relative_check = tk.Checkbutton(options_frame2, text="Project Relative", variable=project_relative_var, command=validate_inputs)
project_relative_check.grid(row=0, column=1, padx=5, sticky="w")
ToolTip(project_relative_check, "When enabled, the Output Folder is treated as the ${KIPRJMOD} base for 3D model paths. If no Output Folder is set yet, this stays as a preference and will apply once you choose one.")

v5_check = tk.Checkbutton(options_frame2, text="KiCad v5", variable=v5_var, command=validate_inputs)
v5_check.grid(row=0, column=2, padx=5, sticky="w")
ToolTip(v5_check, "Convert the library to legacy format for KiCad version 5.x.")

debug_check = tk.Checkbutton(options_frame2, text="Debug", variable=debug_var, command=validate_inputs)
debug_check.grid(row=0, column=3, padx=5, sticky="w")
ToolTip(debug_check, "Enabled by default so API and 3D-model failures show the actual CLI output.")

# Run Button
run_button = tk.Button(main_frame, text="Run", command=run_command)
run_button.grid(row=8, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
run_button_tooltip = ToolTip(run_button, "Execute the conversion with the selected options.")

# Command Display (within main_frame)
small_font = tkFont.Font(size=8)
command_display = tk.Text(main_frame, height=2, state='disabled', bg="#f0f0f0", wrap='word', font=small_font)
command_display.grid(row=9, column=0, columnspan=2, padx=5, pady=(3,5), sticky="ew")
ToolTip(command_display, "Shows the exact CLI command the GUI is about to run.")

# Trace variables for live command display and input validation
variables_to_trace = [
    output_var,
    library_name_var,
    destination_mode_var,
    full_var,
    symbol_var,
    footprint_var,
    model3d_var,
    project_relative_var,
    overwrite_var,
    v5_var,
    debug_var,
]

for var in variables_to_trace:
    var.trace_add('write', update_command_display)
    var.trace_add('write', validate_inputs)

lcsc_id_var.trace_add('write', schedule_part_lookup)
lcsc_id_var.trace_add('write', update_command_display)
lcsc_id_var.trace_add('write', validate_inputs)
library_name_var.trace_add('write', on_library_name_change)

# Call on_full_toggle initially so the default Full state is reflected in the UI
on_full_toggle()
schedule_part_lookup()
update_destination_preview()

# Start the main loop
root.mainloop()
