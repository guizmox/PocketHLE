@echo off
setlocal
set "POCKETHLE_CE_GUI=%~dp0..\target\release\pockethle-gui.exe"
if not exist "%POCKETHLE_CE_GUI%" set "POCKETHLE_CE_GUI=%~dp0..\target\debug\pockethle-gui.exe"
if not exist "%POCKETHLE_CE_GUI%" (
    echo Build pockethle-gui first with tools\build-windows.cmd build -p pocket-desktop --release --offline
    exit /b 1
)
start "" "%POCKETHLE_CE_GUI%" --ce-profile "%~dp0..\profiles\wince6-native.json" --windowed
