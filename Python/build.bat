cd /d "%~dp0"

pyinstaller --noconfirm --onefile --windowed --icon "easyeda2kicad_gui.ico" --add-data "easyeda2kicad_gui.ico;." "easyeda2kicad_gui.py"
copy /y "dist\easyeda2kicad_gui.exe" .
rmdir /s /q build
rmdir /s /q dist
del /q "easyeda2kicad_gui.spec"
pause