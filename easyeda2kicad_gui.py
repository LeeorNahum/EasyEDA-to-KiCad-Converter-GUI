import tkinter as tk
from tkinter import filedialog, messagebox
import tkinter.font as tkFont
import subprocess
from pathlib import Path
import os

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
            elif not self.textvariable.get() and not self.focus_get() == self:
                self.put_placeholder()

def run_command():
    # Collect options
    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()
    library_name = library_name_entry.get_real_text().strip()
    create_folder = create_folder_var.get()

    # Get the state of the checkbuttons
    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()
    overwrite = overwrite_var.get()
    v5 = v5_var.get()
    project_relative = project_relative_var.get()
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

    # Build the command
    command = ["easyeda2kicad"]

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

    # Handle output path and library name
    if output_path:
        # If Library Name is provided, construct the output path
        if library_name:
            base_output_path = Path(output_path)
            if create_folder:
                full_output_path = base_output_path / library_name / library_name
            else:
                full_output_path = base_output_path / library_name
            command.extend(["--output", str(full_output_path)])
        else:
            command.extend(["--output", output_path])
    else:
        # Output path is optional; if not provided, the script uses the default path
        pass

    if overwrite:
        command.append("--overwrite")
    if v5:
        command.append("--v5")
    if project_relative:
        command.append("--project-relative")
    if debug:
        command.append("--debug")

    # Format the command string with quotes if necessary
    command_str = ' '.join(f'"{arg}"' if ' ' in arg else arg for arg in command)

    # Display the command in the command display box
    command_display.config(state='normal')
    command_display.delete(1.0, tk.END)
    command_display.insert(tk.END, command_str)
    command_display.config(state='disabled')

    # Run the command
    try:
        print("Running command:", ' '.join(command))
        result = subprocess.run(command, capture_output=True, text=True, shell=False)
        if result.returncode != 0:
            # Show error message with stderr
            error_message = f"An error occurred:\n{result.stderr}"
            if debug:
                # Include detailed stderr in the popup
                messagebox.showerror("Error", error_message)
            else:
                # Show a simplified error message
                messagebox.showerror("Error", "An error occurred during the conversion. Please check your inputs and try again.")
        else:
            # Show success message
            success_message = "Conversion completed successfully."
            if project_relative:
                success_message += "\nNote: 3D model paths are set relative to the project directory (${KIPRJMOD})."
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
    # Collect options
    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()
    library_name = library_name_entry.get_real_text().strip()
    create_folder = create_folder_var.get()

    # Get the state of the checkbuttons
    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()
    overwrite = overwrite_var.get()
    v5 = v5_var.get()
    project_relative = project_relative_var.get()
    debug = debug_var.get()

    # Build the command
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

    if output_path:
        if library_name:
            base_output_path = Path(output_path)
            if create_folder:
                full_output_path = base_output_path / library_name / library_name
            else:
                full_output_path = base_output_path / library_name
            command.extend(["--output", str(full_output_path)])
        else:
            command.extend(["--output", output_path])
    else:
        pass

    if overwrite:
        command.append("--overwrite")
    if v5:
        command.append("--v5")
    if project_relative:
        command.append("--project-relative")
    if debug:
        command.append("--debug")

    # Format the command string with quotes if necessary
    command_str = ' '.join(f'"{arg}"' if ' ' in arg else arg for arg in command)

    # Update the command display box
    command_display.config(state='normal')
    command_display.delete(1.0, tk.END)
    command_display.insert(tk.END, command_str)
    command_display.config(state='disabled')

def validate_inputs(*args):
    errors = []

    lcsc_id = lcsc_entry.get_real_text().strip()
    output_path = output_entry.get_real_text().strip()
    library_name = library_name_entry.get_real_text().strip()

    if not lcsc_id:
        errors.append("LCSC Part # is required.")

    # Check options
    full = full_var.get()
    symbol = symbol_var.get()
    footprint = footprint_var.get()
    model3d = model3d_var.get()

    if not full and not (symbol or footprint or model3d):
        errors.append("Select Full or at least one of Symbol, Footprint, or 3D Model.")

    # Check project-relative association
    project_relative = project_relative_var.get()
    if project_relative and not output_path:
        errors.append("Project-relative option requires an Output Folder.")

    if errors:
        run_button.config(state='disabled', bg='SystemButtonFace', fg='SystemButtonText', text='Run')
        run_button_tooltip.text = "Cannot run due to the following errors:\n- " + "\n- ".join(errors)
    else:
        run_button.config(state='normal', bg='green', fg='white', text='Run')
        run_button_tooltip.text = "Execute the conversion with the selected options."

    # Enable/disable Library Name, Create Folder, and Project Relative based on Output Path
    if output_path:
        library_name_entry.set_state('normal')
        create_folder_check.config(state='normal')
        project_relative_check.config(state='normal')
    else:
        library_name_entry.set_state('disabled')
        create_folder_check.config(state='disabled')
        project_relative_check.config(state='disabled')
        library_name_var.set('')
        create_folder_var.set(False)
        project_relative_var.set(False)

# Create the main window
root = tk.Tk()
root.title("EasyEDA to KiCad Converter GUI")
root.resizable(False, False)
# Set the window icon
try:
    root.iconbitmap('easyeda2kicad_gui.ico')
except Exception as e:
    print(f"Warning: Unable to set icon. {e}")

# Variables
lcsc_id_var = tk.StringVar()
output_var = tk.StringVar()
library_name_var = tk.StringVar()
symbol_var = tk.BooleanVar()
footprint_var = tk.BooleanVar()
model3d_var = tk.BooleanVar()
full_var = tk.BooleanVar()
overwrite_var = tk.BooleanVar()
v5_var = tk.BooleanVar()
project_relative_var = tk.BooleanVar()
debug_var = tk.BooleanVar()
create_folder_var = tk.BooleanVar()

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

# Output Folder and Browse Button
output_frame = tk.Frame(main_frame)
output_frame.grid(row=1, column=0, columnspan=2, sticky="ew", **padding_options)
output_frame.grid_columnconfigure(0, weight=1)

output_entry = PlaceholderEntry(output_frame, placeholder="Output Folder (Optional)", textvariable=output_var)
output_entry.grid(row=0, column=0, sticky="ew")
ToolTip(output_entry, "Select the base folder where the library will be saved. If not specified, a default folder will be used.")

browse_button = tk.Button(output_frame, text="Browse", command=browse_output)
browse_button.grid(row=0, column=1, padx=(5,0))
ToolTip(browse_button, "Click to browse and select the output folder.")

# Library Name and Create Folder Checkbox
library_frame = tk.Frame(main_frame)
library_frame.grid(row=2, column=0, columnspan=2, sticky="ew", **padding_options)
library_frame.grid_columnconfigure(0, weight=1)

library_name_entry = PlaceholderEntry(library_frame, placeholder="Library Name", textvariable=library_name_var, state='disabled')
library_name_entry.grid(row=0, column=0, sticky="ew")
ToolTip(library_name_entry, "Enter the name of the library (e.g., MyLib). This is optional unless Output Folder is specified.")

create_folder_check = tk.Checkbutton(library_frame, text="Create Folder", variable=create_folder_var, state='disabled', command=validate_inputs)
create_folder_check.grid(row=0, column=1, padx=(5,0))
ToolTip(create_folder_check, "Check to create a new folder named after the library within the selected Output Folder.")

# Options: Full, Symbol, Footprint, 3D Model
options_frame = tk.LabelFrame(main_frame, text="Options")
options_frame.grid(row=3, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
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
options_frame2.grid(row=4, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
options_frame2.grid_columnconfigure((0,1,2,3), weight=1)

overwrite_check = tk.Checkbutton(options_frame2, text="Overwrite", variable=overwrite_var, command=validate_inputs)
overwrite_check.grid(row=0, column=0, padx=5, sticky="w")
ToolTip(overwrite_check, "Overwrite existing library files if they already exist.")

project_relative_check = tk.Checkbutton(options_frame2, text="Project Relative", variable=project_relative_var, command=validate_inputs, state='disabled')
project_relative_check.grid(row=0, column=1, padx=5, sticky="w")
ToolTip(project_relative_check, "Set the 3D model paths to be relative to the project directory (${KIPRJMOD}), ensuring project portability. Requires an Output Folder.")

v5_check = tk.Checkbutton(options_frame2, text="KiCad v5", variable=v5_var, command=validate_inputs)
v5_check.grid(row=0, column=2, padx=5, sticky="w")
ToolTip(v5_check, "Convert the library to legacy format for KiCad version 5.x.")

debug_check = tk.Checkbutton(options_frame2, text="Debug", variable=debug_var, command=validate_inputs)
debug_check.grid(row=0, column=3, padx=5, sticky="w")
ToolTip(debug_check, "Enable debug mode to display detailed error messages. Useful for troubleshooting.")

# Run Button
run_button = tk.Button(main_frame, text="Run", command=run_command)
run_button.grid(row=5, column=0, columnspan=2, padx=5, pady=5, sticky="ew")
run_button_tooltip = ToolTip(run_button, "Execute the conversion with the selected options.")

# Command Display (within main_frame)
small_font = tkFont.Font(size=8)
command_display = tk.Text(main_frame, height=2, state='disabled', bg="#f0f0f0", wrap='word', font=small_font)
command_display.grid(row=6, column=0, columnspan=2, padx=5, pady=(3,5), sticky="ew")
ToolTip(command_display, "Displays the command being executed.")

# Trace variables for live command display and input validation
variables_to_trace = [
    lcsc_id_var,
    output_var,
    library_name_var,
    full_var,
    symbol_var,
    footprint_var,
    model3d_var,
    project_relative_var,
    overwrite_var,
    v5_var,
    debug_var,
    create_folder_var,
]

for var in variables_to_trace:
    var.trace_add('write', update_command_display)
    var.trace_add('write', validate_inputs)

# Call validate_inputs initially to set the correct state of the Run button
validate_inputs()
update_command_display()

root.mainloop()
